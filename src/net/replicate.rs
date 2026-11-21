//! Replication you declare: mark component types as replicated, mark
//! entities, and the engine keeps every client's world in step with the
//! server's.
//!
//! The server is the authority. Every tick it sends each client what
//! changed among the entities that client should see ([interest](Interest)),
//! as small differences from what that client already acknowledged (delta
//! compression), within a bandwidth budget per client, most important
//! first. Clients get the entities spawned, updated and despawned for them
//! as [`Replica`]s. A client's own entities can be **predicted**: its input
//! moves them at once, and the server's state corrects them when it
//! disagrees. Reliable **events** go both ways.
//!
//! ```
//! use ponko::net::{Quantize, Replicate, Replicated, Replication};
//! use ponko::prelude::*;
//!
//! #[derive(Component, Replicate, Clone, Default, PartialEq, Debug)]
//! struct Health {
//!     #[replicate(min = 0.0, max = 100.0, step = 0.5)]
//!     points: f32,
//! }
//!
//! let replication = Replication::new().component::<Health>();
//! let world = Game::new("server", 1, 1)
//!     .replication(replication)
//!     .run_headless(3, |ctx| {
//!         ctx.world.spawn((Health { points: 80.0 }, Replicated::new()));
//!     });
//! # let _ = world;
//! ```
//!
//! The same declarations must be made on the server and on every client
//! (same types, in the same order): the server checks a hash of them when
//! a client joins and refuses one that differs.
//!
//! Which side a game is: without a message endpoint, or with one that
//! hosts, it is the server (offline, a single-player game runs the same
//! code); with one that joined, it is a client.

use std::any::TypeId;
use std::sync::Arc;

use glam::Vec3;
use ponko_ecs::{Component, Entity, World};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, ErrorKind};
use crate::events::Events;
use crate::net::{Interest, InterestConfig, Interpolate, NetMessages, PeerId, SnapshotConfig};

mod bits;
mod client;
mod fields;
mod inputs;
mod kinds;
mod server;
mod wire;

pub use fields::{Fields, Quantize, Replicate, ReplicateField, replicated_bits};
pub use kinds::{RawField, RawType};
/// `#[derive(Replicate)]`: see [`Replicate`](trait@Replicate).
pub use ponko_macros::Replicate;

use inputs::{InputStream, Inputs, Predicted};
use kinds::{Kind, Raw};

/// Bandwidth a server sends each client at most, by default: 64 KB/s.
pub const DEFAULT_BUDGET: u32 = 64_000;

/// A replicated entity's id: the same on the server and every client.
/// [`Replicated::net_id`] on the server, [`Replica::id`] on clients, and
/// [`Replication::entity`] finds the entity of an id on either side.
///
/// A component can refer to another entity by its `NetId` (a target, an
/// owner); it travels as a number, in components and in events alike.
///
/// ```
/// let id = ponko::net::NetId::from_index(7);
/// assert_eq!(id.index(), 7);
/// assert_eq!(serde_json::to_string(&id).unwrap(), "7");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct NetId(pub(crate) u32);

impl NetId {
    /// The id with this index (ids count up from 0 and are reused once
    /// every client forgot the entity that had one).
    #[must_use]
    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// The id's index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl ReplicateField for NetId {
    fn replicate_field(&mut self, name: &str, quantize: Quantize, fields: &mut Fields<'_>) {
        fields.u32(name, &mut self.0, quantize);
    }
}

/// Marks an entity for replication (server side). Its replicated
/// components go to every client that should see it, and its despawn
/// (or the marker's removal) despawns it there.
///
/// ```
/// use ponko::net::{PeerId, Replicated};
/// let npc = Replicated::new();
/// assert_eq!(npc.owner(), None);
/// let player = Replicated::owned_by(PeerId::from_bits(3)).with_priority(4.0);
/// assert_eq!(player.owner(), Some(PeerId::from_bits(3)));
/// assert_eq!(player.priority(), 4.0);
/// ```
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Replicated {
    owner: Option<PeerId>,
    priority: f32,
    global: bool,
    id: Option<u32>,
}

impl Default for Replicated {
    fn default() -> Self {
        Self::new()
    }
}

impl Replicated {
    /// An entity the server owns.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            owner: None,
            priority: 1.0,
            global: false,
            id: None,
        }
    }

    /// An entity `peer` owns: that client's input moves it
    /// ([`Replication::predicted`]), it is sent to that client in every
    /// packet whatever the budget, and the client sees it as
    /// [`Replica::is_mine`]. [`PeerId::HOST`] is the host's own player.
    #[must_use]
    pub const fn owned_by(peer: PeerId) -> Self {
        Self {
            owner: Some(peer),
            ..Self::new()
        }
    }

    /// How much this entity matters when the budget can't send everything:
    /// an entity that waited gains its priority every tick until it is
    /// sent. Default 1; 0 sends it only when nothing else waits.
    #[must_use]
    pub const fn with_priority(mut self, priority: f32) -> Self {
        self.priority = priority;
        self
    }

    /// Sent to every client, whatever [interest](Replication::interest)
    /// says (a scoreboard, the match clock).
    #[must_use]
    pub const fn global(mut self) -> Self {
        self.global = true;
        self
    }

    /// Who owns it.
    #[must_use]
    pub const fn owner(&self) -> Option<PeerId> {
        self.owner
    }

    /// Hands the entity to another owner (or back to the server with
    /// `None`). Clients see a new authority epoch ([`Replica::epoch`]), so
    /// the old owner's predictions are dropped.
    pub fn set_owner(&mut self, owner: Option<PeerId>) {
        self.owner = owner;
    }

    /// Its priority.
    #[must_use]
    pub const fn priority(&self) -> f32 {
        self.priority
    }

    /// Changes its priority.
    pub fn set_priority(&mut self, priority: f32) {
        self.priority = priority;
    }

    /// Whether it ignores interest.
    #[must_use]
    pub const fn is_global(&self) -> bool {
        self.global
    }

    /// Makes it ignore interest (every client gets it), or not.
    ///
    /// ```
    /// let mut r = ponko::net::Replicated::new();
    /// r.set_global(true);
    /// assert!(r.is_global());
    /// ```
    pub fn set_global(&mut self, global: bool) {
        self.global = global;
    }

    /// Its net id, once the server gave it one (at the end of the tick it
    /// was spawned in).
    #[must_use]
    pub fn net_id(&self) -> Option<NetId> {
        self.id.map(NetId)
    }
}

/// A client's copy of a server entity, spawned and despawned by the
/// engine. Its replicated components arrive with it.
///
/// ```
/// use ponko::net::Replica;
/// use ponko::prelude::*;
/// fn count_mine(ctx: &mut Ctx) -> usize {
///     ctx.world.query::<&Replica>().iter().filter(|r| r.is_mine()).count()
/// }
/// ```
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replica {
    id: NetId,
    owner: Option<PeerId>,
    epoch: u16,
    mine: bool,
}

impl Replica {
    /// Its net id.
    #[must_use]
    pub const fn id(&self) -> NetId {
        self.id
    }

    /// Who owns it.
    #[must_use]
    pub const fn owner(&self) -> Option<PeerId> {
        self.owner
    }

    /// The authority epoch: bumped on the server every time the owner
    /// changes ([`Authority`](crate::net::Authority)'s rule).
    #[must_use]
    pub const fn epoch(&self) -> u16 {
        self.epoch
    }

    /// Whether this client owns it.
    #[must_use]
    pub const fn is_mine(&self) -> bool {
        self.mine
    }
}

/// An event that came over the network, in
/// [`ctx.events`](crate::ctx::Ctx::events) as `NetEvent<T>` for every type
/// declared with [`Replication::event`].
///
/// ```
/// use ponko::net::NetEvent;
/// use ponko::prelude::*;
/// struct Chat(String);
/// fn show(ctx: &mut Ctx) {
///     for chat in ctx.events.read::<NetEvent<Chat>>() {
///         println!("{:?} at tick {}: {}", chat.from, chat.tick, chat.event.0);
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct NetEvent<T> {
    /// Who sent it: [`PeerId::HOST`] for the server.
    pub from: PeerId,
    /// From the server: its tick when it sent the event. From a client:
    /// the newest server tick that client had seen (what it was looking
    /// at), for judging what it aimed at.
    pub tick: u64,
    /// The event.
    pub event: T,
}

/// An event declared at run time with [`Replication::raw_event`] (from C or
/// C++): plain bytes, as the sender gave them, read as
/// `NetEvent<RawEvent>` from [`ctx.events`](crate::ctx::Ctx::events). Every
/// raw event type arrives there; [`name`](Self::name) says which one it is.
///
/// ```
/// use ponko::net::{NetEvent, RawEvent};
/// use ponko::prelude::*;
/// fn hits(ctx: &mut Ctx) -> Vec<Vec<u8>> {
///     ctx.events
///         .read::<NetEvent<RawEvent>>()
///         .iter()
///         .filter(|e| e.event.name() == "hit")
///         .map(|e| e.event.bytes.clone())
///         .collect()
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEvent {
    name: Arc<str>,
    /// The bytes, exactly as sent.
    pub bytes: Vec<u8>,
}

impl RawEvent {
    /// The name its type was declared with.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The longest raw event, in bytes: a reliable message
/// ([`MAX_MESSAGE_BYTES`](crate::net::MAX_MESSAGE_BYTES)) less the event's
/// header.
pub const MAX_RAW_EVENT_BYTES: usize = crate::net::MAX_MESSAGE_BYTES - 16;

/// What the server sent one client ([`Replication::peer_stats`]).
///
/// ```
/// assert_eq!(ponko::net::PeerStats::default().bytes_sent, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerStats {
    /// State packets sent.
    pub packets_sent: u64,
    /// Their bytes (payload, without the transport's headers).
    pub bytes_sent: u64,
    /// State packets the client acknowledged.
    pub packets_acked: u64,
    /// State packets never acknowledged within the window of 64.
    pub packets_lost: u64,
    /// Entity records sent (creates, full states and deltas). A change goes
    /// once; it goes again only if the client hasn't acknowledged it within
    /// the resend timeout (the measured round trip plus a margin), or if it
    /// changed again meanwhile.
    pub records_sent: u64,
    /// Spawns sent.
    pub creates_sent: u64,
    /// Full states sent (no baseline the client surely still has).
    pub fulls_sent: u64,
    /// Deltas against an acknowledged baseline.
    pub deltas_sent: u64,
    /// Despawns sent (repeated after the resend timeout until
    /// acknowledged).
    pub destroys_sent: u64,
    /// Changed entities left for a later tick by the budget, summed over
    /// ticks.
    pub deferred: u64,
}

/// What a client received and predicted ([`Replication::client_stats`]).
///
/// ```
/// assert_eq!(ponko::net::ClientStats::default().corrections, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientStats {
    /// State packets received.
    pub packets_received: u64,
    /// Their bytes.
    pub bytes_received: u64,
    /// Packets that arrived after a newer one: of those, only records newer
    /// than this client's state of their entity were applied (none, if it
    /// names baselines this client no longer keeps).
    pub packets_stale: u64,
    /// Packets that didn't decode (changed nothing, weren't acknowledged).
    pub packets_bad: u64,
    /// Entity records applied.
    pub records_applied: u64,
    /// Replicas spawned.
    pub spawned: u64,
    /// Replicas despawned.
    pub despawned: u64,
    /// Packets sent to the server (acknowledgements and inputs).
    pub packets_sent: u64,
    /// Their bytes.
    pub bytes_sent: u64,
    /// Predictions the server confirmed.
    pub confirmed: u64,
    /// Predictions the server corrected.
    pub corrections: u64,
    /// Inputs replayed after corrections.
    pub replayed: u64,
}

/// Which side of a replication this game is ([`Replication::role`]).
///
/// ```
/// assert_ne!(ponko::net::ReplicationRole::Server, ponko::net::ReplicationRole::Client);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplicationRole {
    /// No replication, or no tick ran yet.
    #[default]
    Off,
    /// The authority: no message endpoint, or one that hosts.
    Server,
    /// Joined a server.
    Client,
}

type PositionFn = Arc<dyn Fn(&World, Entity) -> Option<Vec3> + Send + Sync>;

/// An event type declared with [`Replication::event`] or
/// [`Replication::raw_event`].
struct EventType {
    name: String,
    delivery: Delivery,
}

/// How an event type's payloads become events.
enum Delivery {
    /// Decoded into a Rust type.
    Typed {
        type_id: TypeId,
        deliver: fn(&mut Events, PeerId, u64, &[u8]) -> bool,
    },
    /// Delivered as they are, as [`RawEvent`]s named this.
    Raw(Arc<str>),
}

impl EventType {
    fn is_type(&self, id: TypeId) -> bool {
        matches!(self.delivery, Delivery::Typed { type_id, .. } if type_id == id)
    }

    fn is_raw(&self, name: &str) -> bool {
        matches!(&self.delivery, Delivery::Raw(n) if &**n == name)
    }
}

/// Where an outgoing event goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Server,
    Client(PeerId),
    Clients,
}

/// Events queued at most before the session can take them.
const OUTBOX_LIMIT: usize = 4096;

/// The game's replication: what is replicated (declared once, the same on
/// every side), and the running session: [`ctx.net.replication`](crate::net::Networking::replication).
///
/// Off by default ([`Replication::default`]), and then free: the engine's
/// hooks return at once and nothing travels. [`Replication::new`] starts
/// the declarations; [`Game::replication`](crate::game::Game::replication)
/// gives them to the game.
///
/// ```
/// use ponko::net::{Replicate, Replication};
/// use ponko::prelude::*;
///
/// #[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
/// struct Mover {
///     #[replicate(min = -500.0, max = 500.0, step = 0.01)]
///     position: Vec2,
/// }
///
/// #[derive(Replicate, Clone, Default, PartialEq, Debug)]
/// struct Move {
///     #[replicate(min = -1.0, max = 1.0, step = 0.01)]
///     direction: Vec2,
/// }
///
/// let replication = Replication::new()
///     .input::<Move>()
///     .predicted::<Mover, Move>(|m, input| m.position += input.direction * 0.1)
///     .budget(32_000);
/// assert!(replication.is_active());
/// assert!(!Replication::default().is_active());
/// ```
#[derive(Default)]
pub struct Replication {
    inner: Option<Box<Inner>>,
}

struct Inner {
    kinds: Vec<Kind>,
    stride: usize,
    inputs: Option<Box<dyn InputStream>>,
    events: Vec<EventType>,
    budget: u32,
    interest: Option<InterestConfig>,
    position: PositionFn,
    hash: u64,
    role: ReplicationRole,
    server: server::Server,
    client: client::Client,
    outbox: Vec<(Target, u16, u64, Vec<u8>)>,
    inbox: Vec<(u16, PeerId, u64, Vec<u8>)>,
    hz: f64,
    tick: u64,
    /// Client: this game's clock, in seconds of ticks.
    local_time: f64,
    peers: Vec<PeerId>,
    message: Vec<u8>,
}

impl std::fmt::Debug for Replication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            None => f.write_str("Replication(off)"),
            Some(inner) => f
                .debug_struct("Replication")
                .field("role", &inner.role)
                .field("kinds", &inner.kinds.iter().map(|k| &k.name).collect::<Vec<_>>())
                .field("events", &inner.events.iter().map(|e| &e.name).collect::<Vec<_>>())
                .field("budget", &inner.budget)
                .finish_non_exhaustive(),
        }
    }
}

/// Where an entity is, for interest: its 3D transform's position (feature
/// `3d`), else its 2D transform's (on the ground plane z = 0).
fn default_position(world: &World, e: Entity) -> Option<Vec3> {
    #[cfg(feature = "3d")]
    if let Some(t) = world.get::<crate::d3::Transform>(e) {
        return Some(t.position);
    }
    world.get::<crate::d2::Transform>(e).map(|t| t.position.extend(0.0))
}

impl Inner {
    fn new() -> Self {
        Self {
            kinds: Vec::new(),
            stride: 0,
            inputs: None,
            events: Vec::new(),
            budget: DEFAULT_BUDGET,
            interest: None,
            position: Arc::new(default_position),
            hash: 0,
            role: ReplicationRole::Off,
            server: server::Server::default(),
            client: client::Client::default(),
            outbox: Vec::new(),
            inbox: Vec::new(),
            hz: 60.0,
            tick: 0,
            local_time: 0.0,
            peers: Vec::new(),
            message: Vec::new(),
        }
    }

    fn add_kind(&mut self, mut kind: Kind) -> usize {
        if let Some(type_id) = kind.type_id {
            assert!(
                !self.kinds.iter().any(|k| k.type_id == Some(type_id)),
                "Replication: `{}` is declared twice; declare each component once",
                kind.name
            );
        }
        assert!(
            self.kinds.len() < wire::MAX_KINDS,
            "Replication: at most {} component types can be replicated; put fields that change together into one component",
            wire::MAX_KINDS
        );
        kind.offset = self.stride;
        self.stride += kind.words();
        self.kinds.push(kind);
        self.kinds.len() - 1
    }

    fn kind_of<T: 'static>(&self) -> Option<usize> {
        self.kinds.iter().position(|k| k.type_id == Some(TypeId::of::<T>()))
    }

    fn inputs_of<I: Replicate>(&mut self) -> &mut Inputs<I> {
        let name = crate::reflect::short_name(std::any::type_name::<I>());
        let inputs = self.inputs.get_or_insert_with(|| Box::new(Inputs::<I>::new()));
        let found = inputs.name().to_string();
        match inputs.as_any_mut().downcast_mut::<Inputs<I>>() {
            Some(i) => i,
            None => panic!(
                "Replication: the input type is `{found}`, not `{name}`; a game has one input type (put every control in it)"
            ),
        }
    }

    /// A hash of everything both sides must declare alike.
    fn schema_hash(&self) -> u64 {
        let mut h = Fnv::new();
        // 2: events in MessagePack (1 sent JSON).
        h.str("ponko-replication-2");
        for kind in &self.kinds {
            h.str(&kind.name);
            for spec in &kind.specs {
                h.str(&spec.name);
                h.u64(u64::from(spec.bits));
            }
        }
        if let Some(inputs) = &self.inputs {
            h.str(inputs.name());
            for spec in inputs.specs() {
                h.str(&spec.name);
                h.u64(u64::from(spec.bits));
            }
            h.u64(inputs.predicted());
        }
        for event in &self.events {
            if matches!(event.delivery, Delivery::Raw(_)) {
                // A raw event never matches a typed one of the same name.
                h.str("raw event");
            }
            h.str(&event.name);
        }
        h.0
    }

    fn switch(&mut self, role: ReplicationRole, world: &mut World) {
        if self.role == ReplicationRole::Client {
            self.client.reset(world, self.inputs.as_deref_mut());
        }
        self.server = server::Server::default();
        self.server.interest = self.interest.clone().map(Interest::new);
        self.client = client::Client::default();
        if let Some(i) = &mut self.inputs {
            i.reset();
        }
        self.outbox.clear();
        self.inbox.clear();
        self.hash = self.schema_hash();
        self.role = role;
    }

    fn deliver(&mut self, events: &mut Events) {
        for (index, from, tick, bytes) in self.inbox.drain(..) {
            match self.events.get(usize::from(index)).map(|t| &t.delivery) {
                Some(Delivery::Typed { deliver, .. }) => {
                    let _ = deliver(events, from, tick, &bytes);
                }
                Some(Delivery::Raw(name)) => events.send(NetEvent {
                    from,
                    tick,
                    event: RawEvent {
                        name: name.clone(),
                        bytes,
                    },
                }),
                None => {}
            }
        }
    }

    // Server.

    fn server_begin(&mut self, endpoint: Option<&mut NetMessages>, events: &mut Events) {
        if let Some(m) = endpoint {
            // Who is connected: new peers get a hello, gone ones are forgotten.
            self.peers.clear();
            self.peers.extend(m.peer_ids());
            for i in 0..self.peers.len() {
                let peer = self.peers[i];
                if !self.server.clients.contains_key(&peer) {
                    self.server.add_client(peer);
                    self.message.clear();
                    self.message.push(wire::HELLO);
                    self.message.extend_from_slice(&self.hash.to_le_bytes());
                    self.message.extend_from_slice(&peer.to_bits().to_le_bytes());
                    self.message.extend_from_slice(&self.hz.to_le_bytes());
                    let _ = m.send_internal(peer, true, &self.message);
                }
            }
            let gone: Vec<PeerId> = self
                .server
                .clients
                .keys()
                .filter(|p| !self.peers.contains(p))
                .copied()
                .collect();
            for peer in gone {
                self.server.remove_client(peer);
                if let Some(i) = &mut self.inputs {
                    i.forget(peer);
                }
            }
            while let Some((peer, reliable, bytes)) = m.next_internal() {
                if reliable {
                    self.server_message(m, peer, &bytes);
                } else {
                    let _ = self
                        .server
                        .receive(peer, &bytes, self.stride, self.tick, self.inputs.as_deref_mut());
                }
            }
        }
        self.deliver(events);
    }

    fn server_message(&mut self, m: &mut NetMessages, peer: PeerId, bytes: &[u8]) {
        let Some((&kind, rest)) = bytes.split_first() else {
            return;
        };
        let Some(conn) = self.server.clients.get_mut(&peer) else {
            return;
        };
        match kind {
            wire::READY => {
                let theirs = rest
                    .get(..8)
                    .map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])));
                if theirs == Some(self.hash) {
                    conn.ready = true;
                } else {
                    conn.rejected = true;
                    let mut reply = vec![wire::REJECT];
                    reply.extend_from_slice(
                        b"the server replicates different components, inputs or events (a different build?)",
                    );
                    let _ = m.send_internal(peer, true, &reply);
                }
            }
            wire::EVENT if conn.ready => {
                if let Some((index, tick, payload)) = read_event(rest)
                    && self.inbox.len() < OUTBOX_LIMIT
                {
                    self.inbox.push((index, peer, tick, payload.to_vec()));
                }
            }
            _ => {}
        }
    }

    fn server_end(&mut self, endpoint: Option<&mut NetMessages>, world: &mut World) {
        self.server.refresh(world);
        if let Some(inputs) = &mut self.inputs {
            inputs.server_tick(world, self.server.owned());
        }
        let Some(m) = endpoint else {
            // Offline: only the host's own deliveries.
            self.server.idle();
            self.flush_local();
            return;
        };
        let any_ready = self.server.clients.values().any(|c| c.ready);
        if any_ready {
            self.server.extract(world, &self.kinds, self.stride, &*self.position);
        } else {
            self.server.idle();
        }
        for (target, index, tick, payload) in std::mem::take(&mut self.outbox) {
            match target {
                Target::Server | Target::Client(PeerId::HOST) => {
                    self.inbox.push((index, PeerId::HOST, tick, payload));
                }
                Target::Client(peer) => self.send_event(m, peer, index, tick, &payload),
                Target::Clients => {
                    self.peers.clear();
                    self.peers.extend(self.server.clients.keys().copied());
                    for i in 0..self.peers.len() {
                        self.send_event(m, self.peers[i], index, tick, &payload);
                    }
                }
            }
        }
        if any_ready {
            let settings = server::SendSettings {
                budget: self.budget,
                hz: self.hz,
                tick: self.tick,
            };
            self.server
                .send(m, &self.kinds, self.stride, settings, self.inputs.as_deref());
        }
    }

    /// Sends an event to a client that joined.
    fn send_event(&mut self, m: &mut NetMessages, peer: PeerId, index: u16, tick: u64, payload: &[u8]) {
        if self.server.clients.get(&peer).is_some_and(|c| c.ready) {
            write_event(&mut self.message, index, tick, payload);
            let _ = m.send_internal(peer, true, &self.message);
        }
    }

    fn flush_local(&mut self) {
        for (target, index, tick, payload) in self.outbox.drain(..) {
            if matches!(target, Target::Server | Target::Client(PeerId::HOST)) {
                self.inbox.push((index, PeerId::HOST, tick, payload));
            }
        }
    }

    // Client.

    fn client_begin(&mut self, m: &mut NetMessages, world: &mut World, events: &mut Events) {
        if self.client.joined && m.peer_ids().next().is_none() {
            // The server is gone: so are its entities.
            self.client.reset(world, self.inputs.as_deref_mut());
        }
        while let Some((peer, reliable, bytes)) = m.next_internal() {
            if peer != PeerId::HOST {
                continue;
            }
            if reliable {
                self.client_message(m, world, &bytes);
            } else if self.client.joined {
                self.client.receive_state(
                    &bytes,
                    &self.kinds,
                    self.stride,
                    world,
                    self.inputs.as_deref_mut(),
                    self.local_time,
                );
            }
        }
        self.deliver(events);
    }

    fn client_message(&mut self, m: &mut NetMessages, world: &mut World, bytes: &[u8]) {
        let Some((&kind, rest)) = bytes.split_first() else {
            return;
        };
        match kind {
            wire::HELLO if rest.len() >= 24 => {
                let word = |at: usize| u64::from_le_bytes(rest[at..at + 8].try_into().unwrap_or([0; 8]));
                let (hash, me, hz) = (word(0), PeerId::from_bits(word(8)), f64::from_bits(word(16)));
                if self.client.joined {
                    // A new session on the same endpoint.
                    self.client.reset(world, self.inputs.as_deref_mut());
                }
                self.client.me = Some(me);
                self.client.hz = if hz.is_finite() && hz > 0.0 { hz } else { 60.0 };
                self.message.clear();
                self.message.push(wire::READY);
                self.message.extend_from_slice(&self.hash.to_le_bytes());
                let _ = m.send_internal(PeerId::HOST, true, &self.message);
                if hash == self.hash {
                    self.client.joined = true;
                    self.client.error = None;
                } else {
                    self.client.error = Some(
                        "the server replicates different components, inputs or events (a different build?)".into(),
                    );
                }
            }
            wire::REJECT => {
                self.client.joined = false;
                self.client.error = Some(String::from_utf8_lossy(rest).into_owned());
            }
            wire::EVENT => {
                if let Some((index, tick, payload)) = read_event(rest)
                    && self.inbox.len() < OUTBOX_LIMIT
                {
                    self.inbox.push((index, PeerId::HOST, tick, payload.to_vec()));
                }
            }
            _ => {}
        }
    }

    fn client_end(&mut self, m: &mut NetMessages, world: &mut World) {
        if !self.client.joined {
            return;
        }
        for (target, index, tick, payload) in std::mem::take(&mut self.outbox) {
            if target == Target::Server {
                write_event(&mut self.message, index, tick, &payload);
                let _ = m.send_internal(PeerId::HOST, true, &self.message);
            }
        }
        self.client.send(m, world, self.inputs.as_deref_mut());
        self.client.sample(world, &self.kinds, self.local_time);
    }

    fn queue_event<T: Serialize + 'static>(&mut self, target: Target, event: &T) {
        let Some(index) = self.events.iter().position(|e| e.is_type(TypeId::of::<T>())) else {
            panic!(
                "Replication: `{0}` isn't a declared event; add `.event::<{0}>()` to the Replication",
                crate::reflect::short_name(std::any::type_name::<T>())
            );
        };
        if self.outbox.len() >= OUTBOX_LIMIT {
            return;
        }
        match encode_event(event) {
            Ok(payload) => self.queue_payload(target, index, payload),
            Err(e) => panic!(
                "Replication: event `{}` doesn't serialize: {e}",
                std::any::type_name::<T>()
            ),
        }
    }

    fn queue_raw(&mut self, target: Target, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let Some(index) = self.events.iter().position(|e| e.is_raw(name)) else {
            return Err(undeclared_raw(name));
        };
        if bytes.len() > MAX_RAW_EVENT_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "Replication: raw event `{name}` has {} bytes, more than {MAX_RAW_EVENT_BYTES}",
                    bytes.len()
                ),
            ));
        }
        if self.outbox.len() >= OUTBOX_LIMIT {
            return Err(Error::new(
                ErrorKind::Limit,
                format!("Replication: {OUTBOX_LIMIT} events wait to be sent already; raw event `{name}` was dropped"),
            ));
        }
        self.queue_payload(target, index, bytes.to_vec());
        Ok(())
    }

    /// Whether `name` is a declared raw event.
    fn check_raw(&self, name: &str) -> Result<(), Error> {
        match self.events.iter().any(|e| e.is_raw(name)) {
            true => Ok(()),
            false => Err(undeclared_raw(name)),
        }
    }

    fn queue_payload(&mut self, target: Target, index: usize, payload: Vec<u8>) {
        let tick = match self.role {
            ReplicationRole::Client => self.client.server_tick,
            _ => self.tick,
        };
        self.outbox.push((target, index as u16, tick, payload));
    }
}

fn undeclared_raw(name: &str) -> Error {
    Error::new(
        ErrorKind::NotFound,
        format!("Replication: `{name}` isn't a declared raw event; add `.raw_event(\"{name}\")` to the Replication"),
    )
}

impl Inner {
    fn declare_event(&mut self, name: String, delivery: Delivery) {
        assert!(
            self.events.len() < usize::from(u16::MAX),
            "Replication: at most {} event types can be declared; put related events into one enum",
            u16::MAX
        );
        self.events.push(EventType { name, delivery });
    }
}

/// An event's payload: MessagePack, with fields by position (no names),
/// so a small event takes a few bytes.
fn encode_event<T: Serialize>(event: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(event)
}

/// An `EVENT` message: the kind, the event type and the tick as varints,
/// then the payload.
fn write_event(out: &mut Vec<u8>, index: u16, tick: u64, payload: &[u8]) {
    out.clear();
    out.push(wire::EVENT);
    push_varint(out, u64::from(index));
    push_varint(out, tick);
    out.extend_from_slice(payload);
}

fn read_event(bytes: &[u8]) -> Option<(u16, u64, &[u8])> {
    let (index, bytes) = take_varint(bytes)?;
    let (tick, bytes) = take_varint(bytes)?;
    Some((u16::try_from(index).ok()?, tick, bytes))
}

/// LEB128: seven bits a byte, low first; the top bit says more follow.
fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn take_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().enumerate().take(10) {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}

fn deliver_event<T: DeserializeOwned + Send + Sync + 'static>(
    events: &mut Events,
    from: PeerId,
    tick: u64,
    bytes: &[u8],
) -> bool {
    match rmp_serde::from_slice::<T>(bytes) {
        Ok(event) => {
            events.send(NetEvent { from, tick, event });
            true
        }
        Err(_) => false,
    }
}

/// FNV-1a, for the schema hash: stable across builds and platforms.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn byte(&mut self, b: u8) {
        self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }
    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        for b in s.bytes() {
            self.byte(b);
        }
    }
}

impl Replication {
    /// An empty declaration: add components, inputs and events to it.
    /// ```
    /// assert!(ponko::net::Replication::new().is_active());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Some(Box::new(Inner::new())),
        }
    }

    fn declare(&mut self) -> &mut Inner {
        self.inner.get_or_insert_with(|| Box::new(Inner::new()))
    }

    /// Replicates component `T`: clients get its value whenever it
    /// changes, set at once when it arrives.
    ///
    /// # Panics
    /// If `T` was declared already, or past 64 component types.
    ///
    /// ```
    /// use ponko::net::{Replicate, Replication};
    /// use ponko::prelude::*;
    /// #[derive(Component, Replicate, Clone, Default)]
    /// struct Score(#[replicate(bits = 16)] u32);
    /// let r = Replication::new().component::<Score>();
    /// # let _ = r;
    /// ```
    #[must_use]
    pub fn component<T: Replicate + Component>(mut self) -> Self {
        self.declare().add_kind(kinds::typed_kind::<T>());
        self
    }

    /// Replicates component `T`, shown on clients between the values
    /// received, 0.1 s in the past: smooth movement from a few packets per
    /// second. Past the newest value it holds still (nothing more arrives
    /// for an entity that stopped, so it mustn't drift on); a client's own
    /// entities aren't delayed.
    ///
    /// # Panics
    /// As [`component`](Self::component).
    ///
    /// ```
    /// use ponko::net::{Replicate, Replication};
    /// use ponko::prelude::*;
    /// let r = Replication::new().interpolated::<ponko::d2::Transform>();
    /// # let _ = r;
    /// ```
    #[must_use]
    pub fn interpolated<T: Replicate + Component + Interpolate>(self) -> Self {
        self.interpolated_with::<T>(SnapshotConfig {
            max_extrapolation: 0.0,
            ..SnapshotConfig::default()
        })
    }

    /// [`interpolated`](Self::interpolated) with your own delay, buffer
    /// size and extrapolation limit. Predicting past the newest value hides
    /// late packets, but an entity that stops is then shown up to the
    /// limit's worth of movement past where it stopped, until it changes
    /// again.
    ///
    /// # Panics
    /// As [`component`](Self::component), and if `config` isn't valid
    /// ([`SnapshotConfig::validate`]).
    #[must_use]
    pub fn interpolated_with<T: Replicate + Component + Interpolate>(mut self, config: SnapshotConfig) -> Self {
        if let Err(e) = config.validate() {
            panic!("Replication::interpolated_with: {e}");
        }
        self.declare().add_kind(kinds::interpolated_kind::<T>(config));
        self
    }

    /// The game's input type: what a client sends the server every tick
    /// (set it with [`set_input`](Self::set_input)). The server applies each
    /// input once, in order, to the entities that client owns, through the
    /// steps of [`predicted`](Self::predicted) components; systems read the
    /// last one with [`input_of`](Self::input_of).
    ///
    /// # Panics
    /// If another input type was declared already (a game has one).
    #[must_use]
    pub fn input<I: Replicate>(mut self) -> Self {
        self.declare().inputs_of::<I>();
        self
    }

    /// Replicates component `S`, predicted on the client that owns the
    /// entity: `step` applies an input to it, on that client at once and on
    /// the server when the input arrives. When the server's state differs
    /// from what the client predicted for the same input, the client takes
    /// the server's and replays its newer inputs. Other clients see `S` as a
    /// plain component.
    ///
    /// Both sides round the state to its quantization after every step, so
    /// a step that depends only on the state and the input predicts exactly.
    /// Write the whole movement in `step` (walls included): changes other
    /// systems make on the server arrive as corrections.
    ///
    /// # Panics
    /// As [`component`](Self::component) and [`input`](Self::input).
    #[must_use]
    pub fn predicted<S, I>(mut self, step: impl Fn(&mut S, &I) + Send + Sync + 'static) -> Self
    where
        S: Replicate + Component + PartialEq,
        I: Replicate,
    {
        let inner = self.declare();
        let kind = match inner.kind_of::<S>() {
            Some(k) => k,
            None => inner.add_kind(kinds::typed_kind::<S>()),
        };
        let step: inputs::Step<S, I> = Arc::new(step);
        inner.inputs_of::<I>().steps.push(Box::new(Predicted::new(kind, step)));
        self
    }

    /// Declares event type `T`, sent with [`to_server`](Self::to_server),
    /// [`to_client`](Self::to_client) and [`to_clients`](Self::to_clients)
    /// and read as [`NetEvent<T>`] from [`ctx.events`](crate::ctx::Ctx::events).
    /// Events are reliable and ordered, delivered at the start of a tick.
    ///
    /// They travel in binary: MessagePack with fields by position (not by
    /// name), so a small event takes a few bytes. A field the type leaves
    /// out when serializing (`#[serde(skip_serializing_if = ...)]`) shifts
    /// the others, and the receiver drops the event: send it as it is.
    ///
    /// # Panics
    /// If `T` was declared already.
    #[must_use]
    pub fn event<T: Serialize + DeserializeOwned + Send + Sync + 'static>(mut self) -> Self {
        let inner = self.declare();
        let name = crate::reflect::short_name(std::any::type_name::<T>()).to_string();
        assert!(
            !inner.events.iter().any(|e| e.is_type(TypeId::of::<T>())),
            "Replication: event `{name}` is declared twice; declare each event once"
        );
        inner.declare_event(
            name,
            Delivery::Typed {
                type_id: TypeId::of::<T>(),
                deliver: deliver_event::<T>,
            },
        );
        self
    }

    /// Declares an event type by name, whose events are plain bytes (for
    /// C and C++, or any game that encodes its own): sent with
    /// [`to_server_raw`](Self::to_server_raw),
    /// [`to_client_raw`](Self::to_client_raw) and
    /// [`to_clients_raw`](Self::to_clients_raw), read as
    /// `NetEvent<`[`RawEvent`]`>` from [`ctx.events`](crate::ctx::Ctx::events).
    /// Reliable and ordered like [typed events](Self::event) (in one order
    /// with them), delivered at the start of a tick, the bytes unchanged.
    ///
    /// Both sides declare it, in the same place among the events: a raw
    /// event never matches a typed one, even of the same name.
    ///
    /// # Panics
    /// If a raw event of this name was declared already.
    ///
    /// ```
    /// use ponko::net::{NetEvent, RawEvent, Replication};
    /// use ponko::prelude::*;
    ///
    /// let world = Game::new("server", 1, 1)
    ///     .replication(Replication::new().raw_event("ping"))
    ///     .run_headless(3, |ctx| {
    ///         // Offline, the server is its own client: it hears itself.
    ///         ctx.net.replication.to_server_raw("ping", &[1, 2, 3]).unwrap();
    ///         assert!(ctx.net.replication.to_server_raw("pong", &[]).is_err());
    ///     });
    /// # let _ = world;
    /// ```
    #[must_use]
    pub fn raw_event(mut self, name: &str) -> Self {
        let inner = self.declare();
        assert!(
            !inner.events.iter().any(|e| e.is_raw(name)),
            "Replication: raw event `{name}` is declared twice; declare each event once"
        );
        inner.declare_event(name.to_string(), Delivery::Raw(Arc::from(name)));
        self
    }

    /// Bytes per second the server sends each client at most (default
    /// [`DEFAULT_BUDGET`]). When changes don't fit, the most important wait:
    /// each waiting entity gains its [priority](Replicated::with_priority)
    /// (scaled by its interest ring) every tick, and the highest go first.
    /// A client's own entities are always sent.
    #[must_use]
    pub fn budget(mut self, bytes_per_second: u32) -> Self {
        self.declare().budget = bytes_per_second;
        self
    }

    /// Sends each client only the entities near it, nearer ones more often
    /// (rings and rates from `config`; see [`Interest`]). A client's
    /// position is its owned entities' (or what
    /// [`set_viewer`](Self::set_viewer) gave); entities without a position
    /// and [global](Replicated::global) ones go to everyone.
    ///
    /// # Panics
    /// If `config` isn't valid ([`InterestConfig::validate`]).
    #[must_use]
    pub fn interest(mut self, config: InterestConfig) -> Self {
        if let Err(e) = config.validate() {
            panic!("Replication::interest: {e}");
        }
        self.declare().interest = Some(config);
        self
    }

    /// Where an entity is, for interest. By default its `d3::Transform`
    /// (feature `3d`) or `d2::Transform` position.
    #[must_use]
    pub fn position(mut self, position: impl Fn(&World, Entity) -> Option<Vec3> + Send + Sync + 'static) -> Self {
        self.declare().position = Arc::new(position);
        self
    }

    /// Replicates a component registered at run time (from C or C++): plain
    /// bytes of `size`, whose `fields` say where each replicated value is and
    /// how it travels.
    ///
    /// # Panics
    /// If a field lies outside `size` bytes, its quantization doesn't fit
    /// its type, or past 64 component types.
    #[must_use]
    pub fn raw_component(mut self, name: &str, id: ponko_ecs::ComponentId, size: usize, fields: Vec<RawField>) -> Self {
        for f in &fields {
            assert!(
                f.offset + f.ty.size() <= size,
                "Replication::raw_component: field `{}` of `{name}` lies outside its {size} bytes",
                f.name
            );
        }
        let raw = Raw { id, size, fields };
        let specs = raw.schema();
        self.declare().add_kind(Kind {
            name: name.to_string(),
            specs,
            offset: 0,
            type_id: None,
            access: Box::new(raw),
        });
        self
    }

    // The running session.

    /// Whether replication is on (anything was declared).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.is_some()
    }

    /// Which side this game is, from the first tick on.
    #[must_use]
    pub fn role(&self) -> ReplicationRole {
        self.inner.as_ref().map_or(ReplicationRole::Off, |i| i.role)
    }

    /// This side's peer id: [`PeerId::HOST`] on the server, the id the
    /// server gave on a client (once it said hello).
    #[must_use]
    pub fn me(&self) -> Option<PeerId> {
        let inner = self.inner.as_ref()?;
        match inner.role {
            ReplicationRole::Server => Some(PeerId::HOST),
            ReplicationRole::Client => inner.client.me,
            ReplicationRole::Off => None,
        }
    }

    /// Client: whether the server accepted this client's declarations and
    /// sends it state. Always true on the server.
    #[must_use]
    pub fn is_joined(&self) -> bool {
        self.inner.as_ref().is_some_and(|i| match i.role {
            ReplicationRole::Server => true,
            ReplicationRole::Client => i.client.joined,
            ReplicationRole::Off => false,
        })
    }

    /// Client: why the session was refused (different declarations).
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.inner.as_ref()?.client.error.as_deref()
    }

    /// The newest server tick: this game's own on the server, the newest
    /// received on a client.
    #[must_use]
    pub fn server_tick(&self) -> u64 {
        self.inner.as_ref().map_or(0, |i| match i.role {
            ReplicationRole::Client => i.client.server_tick,
            _ => i.tick,
        })
    }

    /// This tick's input of the local player: sent to the server on a
    /// client (and predicted on its entities), applied to the host's own
    /// entities on the server. Set it every tick; a tick without one sends
    /// `I::default()` ("nothing pressed").
    ///
    /// # Panics
    /// If `I` isn't the declared input type ([`input`](Self::input)), or
    /// none was declared.
    pub fn set_input<I: Replicate>(&mut self, input: I) {
        let Some(inner) = self.inner.as_mut().filter(|i| i.inputs.is_some()) else {
            panic!(
                "Replication::set_input: no input type was declared; add `.input::<{}>()` to the Replication",
                crate::reflect::short_name(std::any::type_name::<I>())
            );
        };
        let role = inner.role;
        let inputs = inner.inputs_of::<I>();
        match role {
            ReplicationRole::Client => inputs.pending = Some(input),
            _ => inputs.local = Some(input),
        }
    }

    /// Server: the input of `peer` (or [`PeerId::HOST`]) applied last.
    ///
    /// # Panics
    /// If `I` isn't the declared input type.
    #[must_use]
    pub fn input_of<I: Replicate>(&self, peer: PeerId) -> Option<I> {
        let inputs = self.inner.as_ref()?.inputs.as_ref()?;
        match inputs.as_any().downcast_ref::<Inputs<I>>() {
            Some(i) => i.current.get(&peer).cloned(),
            None => panic!(
                "Replication::input_of: the input type is `{}`, not `{}`; read it with the declared input type",
                inputs.name(),
                std::any::type_name::<I>()
            ),
        }
    }

    /// The entity with net id `id` on this side, if there is one.
    #[must_use]
    pub fn entity(&self, id: NetId) -> Option<Entity> {
        let inner = self.inner.as_ref()?;
        match inner.role {
            ReplicationRole::Client => inner.client.entity(id),
            _ => inner.server.entity(id.0),
        }
    }

    /// Sends `event` to the server (reliable, ordered). On the server
    /// itself (the host's own player, or offline) it arrives there too, at
    /// the start of the next tick.
    ///
    /// # Panics
    /// If `T` isn't a declared event ([`event`](Self::event)), or the value
    /// doesn't serialize (JSON: a map needs string keys).
    pub fn to_server<T: Serialize + 'static>(&mut self, event: &T) {
        self.declare().queue_event(Target::Server, event);
    }

    /// Server: sends `event` to one client. To [`PeerId::HOST`] it reaches
    /// the host's own player (this game, next tick). Does nothing on a client.
    ///
    /// # Panics
    /// As [`to_server`](Self::to_server).
    pub fn to_client<T: Serialize + 'static>(&mut self, peer: PeerId, event: &T) {
        let inner = self.declare();
        if inner.role != ReplicationRole::Client {
            inner.queue_event(Target::Client(peer), event);
        }
    }

    /// Server: sends `event` to every client that joined. Not to the host's
    /// own player: the server knows what it sent. Does nothing on a client.
    ///
    /// # Panics
    /// As [`to_server`](Self::to_server).
    pub fn to_clients<T: Serialize + 'static>(&mut self, event: &T) {
        let inner = self.declare();
        if inner.role != ReplicationRole::Client {
            inner.queue_event(Target::Clients, event);
        }
    }

    /// Sends a raw event's bytes to the server, as [`to_server`](Self::to_server)
    /// does a typed one.
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`] if `name` isn't a declared raw event
    /// ([`raw_event`](Self::raw_event)), [`ErrorKind::Limit`] past
    /// [`MAX_RAW_EVENT_BYTES`] or when 4096 events wait to be sent already.
    /// Nothing is sent then.
    pub fn to_server_raw(&mut self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        match &mut self.inner {
            Some(inner) => inner.queue_raw(Target::Server, name, bytes),
            None => Err(undeclared_raw(name)),
        }
    }

    /// Server: sends a raw event's bytes to one client, as
    /// [`to_client`](Self::to_client) does a typed one. Does nothing on a
    /// client.
    ///
    /// # Errors
    /// As [`to_server_raw`](Self::to_server_raw).
    pub fn to_client_raw(&mut self, peer: PeerId, name: &str, bytes: &[u8]) -> Result<(), Error> {
        match &mut self.inner {
            Some(inner) if inner.role == ReplicationRole::Client => inner.check_raw(name),
            Some(inner) => inner.queue_raw(Target::Client(peer), name, bytes),
            None => Err(undeclared_raw(name)),
        }
    }

    /// Server: sends a raw event's bytes to every client that joined, as
    /// [`to_clients`](Self::to_clients) does a typed one. Does nothing on
    /// a client.
    ///
    /// # Errors
    /// As [`to_server_raw`](Self::to_server_raw).
    pub fn to_clients_raw(&mut self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        match &mut self.inner {
            Some(inner) if inner.role == ReplicationRole::Client => inner.check_raw(name),
            Some(inner) => inner.queue_raw(Target::Clients, name, bytes),
            None => Err(undeclared_raw(name)),
        }
    }

    /// Server: where `peer` looks from, for interest (a spectator camera);
    /// `None` goes back to its owned entities' positions.
    pub fn set_viewer(&mut self, peer: PeerId, position: Option<Vec3>) {
        if let Some(conn) = self.inner.as_mut().and_then(|i| i.server.clients.get_mut(&peer)) {
            conn.viewer = position;
        }
    }

    /// Server: the clients that joined, in id order.
    #[must_use]
    pub fn clients(&self) -> Vec<PeerId> {
        self.inner.as_ref().map_or_else(Vec::new, |i| {
            i.server
                .clients
                .iter()
                .filter(|(_, c)| c.ready)
                .map(|(p, _)| *p)
                .collect()
        })
    }

    /// Server: what was sent to `peer`.
    #[must_use]
    pub fn peer_stats(&self, peer: PeerId) -> Option<PeerStats> {
        Some(self.inner.as_ref()?.server.clients.get(&peer)?.stats)
    }

    /// Client: what was received and predicted.
    #[must_use]
    pub fn client_stats(&self) -> ClientStats {
        self.inner
            .as_ref()
            .map_or_else(ClientStats::default, |i| i.client.stats)
    }

    // Engine hooks.

    /// Start of a simulation tick, before any system: takes what arrived
    /// (state, acknowledgements, inputs, events) and delivers events.
    pub(crate) fn begin_tick(
        &mut self,
        endpoint: Option<&mut NetMessages>,
        world: &mut World,
        events: &mut Events,
        tick: u64,
        hz: f64,
    ) {
        let Some(inner) = &mut self.inner else { return };
        inner.tick = tick;
        inner.hz = hz;
        let role = match &endpoint {
            Some(m) if !m.is_host() => ReplicationRole::Client,
            _ => ReplicationRole::Server,
        };
        if role != inner.role {
            inner.switch(role, world);
        }
        match (role, endpoint) {
            (ReplicationRole::Client, Some(m)) => {
                inner.local_time = tick as f64 / hz;
                inner.client_begin(m, world, events);
            }
            (_, endpoint) => inner.server_begin(endpoint, events),
        }
    }

    /// End of a simulation tick, after every system: the server applies
    /// inputs and sends state; a client predicts and sends its input.
    pub(crate) fn end_tick(&mut self, endpoint: Option<&mut NetMessages>, world: &mut World) {
        let Some(inner) = &mut self.inner else { return };
        match (inner.role, endpoint) {
            (ReplicationRole::Client, Some(m)) => inner.client_end(m, world),
            (ReplicationRole::Server, endpoint) => inner.server_end(endpoint, world),
            _ => {}
        }
    }

    /// Server: the round trip to `peer` the resend timeout is set by, in
    /// ticks (smoothed).
    #[cfg(test)]
    pub(crate) fn peer_rtt_ticks(&self, peer: PeerId) -> Option<f32> {
        self.inner.as_ref()?.server.clients.get(&peer)?.rtt_ticks()
    }

    /// Server: the ids waiting to be read in full.
    #[cfg(test)]
    pub(crate) fn server_pending(&self) -> usize {
        self.inner.as_ref().map_or(0, |i| i.server.pending())
    }

    /// Server: the net ids whose replicated values changed in the last tick.
    #[cfg(test)]
    pub(crate) fn server_changed(&self) -> Vec<u32> {
        let mut ids = self
            .inner
            .as_ref()
            .map_or_else(Vec::new, |i| i.server.changed().to_vec());
        ids.sort_unstable();
        ids
    }

    /// Server: the world tick up to which replication has read the
    /// replicated components' changes. Removal logs are kept from there on.
    pub(crate) fn changes_seen(&self, world: &World) -> Option<ponko_ecs::Tick> {
        let inner = self.inner.as_ref()?;
        (inner.role == ReplicationRole::Server)
            .then(|| inner.server.changes_seen(world))
            .flatten()
    }

    /// Once a frame, after the ticks: shows interpolated kinds at the
    /// frame's time (`alpha` of a tick past the last one).
    pub(crate) fn frame(&mut self, world: &mut World, alpha: f64) {
        let Some(inner) = &mut self.inner else { return };
        if inner.role == ReplicationRole::Client && inner.client.joined {
            let now = inner.local_time + alpha / inner.hz.max(1.0);
            inner.client.sample(world, &inner.kinds, now);
        }
    }
}

// `ctx.net` moves between threads with the game.
const _: fn() = || {
    fn check<T: Send + Sync>() {}
    check::<Replication>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Events a game sends, for their size on the wire.
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Hit {
        target: NetId,
        damage: f32,
        at: glam::Vec2,
        critical: bool,
    }

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Chat {
        text: String,
    }

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    enum Pickup {
        Coin(u32),
        Key { door: u16 },
    }

    /// The `EVENT` message as it was sent before (JSON payload, then a
    /// fixed 2-byte type and 8-byte tick), for comparison.
    fn json_message<T: Serialize>(event: &T) -> usize {
        1 + 2 + 8 + serde_json::to_vec(event).unwrap().len()
    }

    fn binary_message<T: Serialize>(event: &T, tick: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_event(&mut out, 3, tick, &encode_event(event).unwrap());
        out
    }

    fn round_trip<T: Serialize + DeserializeOwned + Send + Sync + Clone + PartialEq + std::fmt::Debug + 'static>(
        event: &T,
        tick: u64,
    ) -> usize {
        let message = binary_message(event, tick);
        let (index, back_tick, payload) = read_event(&message[1..]).expect("a whole message");
        assert_eq!((index, back_tick), (3, tick));
        let mut events = Events::default();
        assert!(deliver_event::<T>(&mut events, PeerId::HOST, back_tick, payload));
        let got: Vec<T> = events.read::<NetEvent<T>>().iter().map(|e| e.event.clone()).collect();
        assert_eq!(got, std::slice::from_ref(event));
        message.len()
    }

    #[test]
    fn events_travel_in_binary_and_arrive_unchanged() {
        let tick = 54_321; // 15 minutes at 60 Hz
        let hit = Hit {
            target: NetId::from_index(417),
            damage: 12.5,
            at: glam::vec2(-153.27, 88.06),
            critical: true,
        };
        let chat = Chat { text: "gg".into() };
        let pickups = [Pickup::Coin(25), Pickup::Key { door: 7 }];
        let rows = [
            ("Hit", round_trip(&hit, tick), json_message(&hit)),
            ("Chat", round_trip(&chat, tick), json_message(&chat)),
            ("Pickup::Coin", round_trip(&pickups[0], tick), json_message(&pickups[0])),
            ("Pickup::Key", round_trip(&pickups[1], tick), json_message(&pickups[1])),
        ];
        for (name, binary, json) in rows {
            println!("{name}: {binary} bytes per event (JSON: {json})");
            assert!(binary < json, "{name}: {binary} bytes in binary, {json} as JSON");
        }
    }

    #[test]
    fn events_with_maps_keyed_by_tuples_travel() {
        // JSON needs string keys, so these couldn't be sent before.
        let grid: std::collections::BTreeMap<(u8, u8), u8> = [((1, 2), 3), ((4, 5), 6)].into();
        assert!(serde_json::to_vec(&grid).is_err());
        round_trip(&grid, 7);
    }

    #[test]
    fn events_that_dont_decode_are_dropped() {
        let message = binary_message(&Chat { text: "gg".into() }, 1);
        let (_, tick, payload) = read_event(&message[1..]).unwrap();
        let mut events = Events::default();
        assert!(!deliver_event::<Hit>(&mut events, PeerId::HOST, tick, payload));
        assert!(events.read::<NetEvent<Hit>>().is_empty());
        assert_eq!(read_event(&[0x80]), None, "a varint cut short");
    }

    /// A field left out when empty: fine in JSON, not by position.
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Sparse {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        note: Option<u32>,
        value: u32,
    }

    #[test]
    fn events_that_leave_out_fields_are_dropped_not_misread() {
        let message = binary_message(&Sparse { note: None, value: 9 }, 1);
        let (_, tick, payload) = read_event(&message[1..]).unwrap();
        let mut events = Events::default();
        assert!(!deliver_event::<Sparse>(&mut events, PeerId::HOST, tick, payload));
        // With the field there, it arrives.
        assert_eq!(
            round_trip(
                &Sparse {
                    note: Some(1),
                    value: 9
                },
                1
            ),
            6
        );
    }

    #[test]
    fn event_varints_round_trip_at_their_edges() {
        for value in [0, 1, 127, 128, 16_383, 16_384, u64::from(u32::MAX), u64::MAX] {
            let mut out = Vec::new();
            push_varint(&mut out, value);
            assert_eq!(take_varint(&out), Some((value, &[][..])), "{value}");
        }
    }
}
