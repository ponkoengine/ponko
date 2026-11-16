//! `ctx.net`: the one place a game's networking lives.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{
    Desync, Interest, LatestWins, Net, NetMessages, NetState, NetStats, Pose, SnapshotSample, SnapshotStream,
    UpdateStamp,
};

/// All of a game's networking: `ctx.net`. Each transport is a field, and the
/// engine drives the ones that are there:
///
/// | Field | What | Driven by the engine |
/// |---|---|---|
/// | [`lockstep`](Self::lockstep) | The deterministic lockstep session ([`Net`]); offline unless [`Game::lockstep`](crate::game::Game::lockstep) gave one | Every simulation tick waits for all inputs, then checks the worlds |
/// | [`messages`](Self::messages) | The byte-message endpoint ([`NetMessages`]) from [`Game::messages`](crate::game::Game::messages) | Polled once per frame before any system (once per tick headless), also while paused |
/// | [`snapshots`](Self::snapshots) | Incoming [`Pose`] snapshots per remote entity ([`NetSnapshots`]) | Nothing to drive: push when a packet arrives, sample when drawing |
/// | [`interest`](Self::interest) | Who receives which entity ([`Interest`]), if the game set one | Nothing: only the game knows its viewers, so it calls [`Interest::update`] on its send tick |
///
/// A single-player game has an offline `lockstep`, no `messages`, no
/// `interest` and no snapshot streams, and pays nothing for them.
///
/// The lockstep calls of a simulation system are also methods here
/// (`ctx.net.set_input(..)` is `ctx.net.lockstep.set_input(..)`), and
/// `&ctx.net` passes where a `&Net` is expected.
///
/// ```
/// use ponko::net::*;
/// use ponko::prelude::*;
/// use std::time::Duration;
///
/// // Two games in one process; over a network it's NetMessages::host / join.
/// let mut ends = NetMessages::loopback(2);
/// let (guest, host) = (ends.pop().unwrap(), ends.pop().unwrap());
/// let host = std::thread::spawn(move || {
///     Game::new("host", 1, 1)
///         .messages(host)
///         .sim_system(|ctx: &mut Ctx| {
///             // The engine polled the endpoint already: read and answer.
///             let messages = ctx.net.messages.as_mut().expect("Game::messages");
///             let mut quit = false;
///             while let Some(event) = messages.next_event() {
///                 match event {
///                     MessageEvent::Message { peer, bytes, .. } => {
///                         messages.send(peer, Lane::Reliable, &bytes).unwrap(); // echo
///                     }
///                     MessageEvent::Disconnected(..) => quit = true,
///                     MessageEvent::Connected(_) => {}
///                 }
///             }
///             if quit {
///                 ctx.quit();
///             }
///             std::thread::sleep(Duration::from_millis(1)); // headless runs don't pace themselves
///         })
///         .run_headless(5000, |_| {})
/// });
/// let world = Game::new("guest", 1, 1)
///     .messages(guest)
///     .sim_system(|ctx: &mut Ctx| {
///         let messages = ctx.net.messages.as_mut().expect("Game::messages");
///         let mut echo = None;
///         while let Some(event) = messages.next_event() {
///             match event {
///                 MessageEvent::Connected(host) => messages.send(host, Lane::Reliable, b"hi").unwrap(),
///                 MessageEvent::Message { bytes, .. } => echo = Some(bytes),
///                 MessageEvent::Disconnected(_, why) => panic!("{why}"),
///             }
///         }
///         if let Some(bytes) = echo {
///             messages.disconnect(PeerId::HOST).unwrap();
///             ctx.world.insert_resource(bytes);
///             ctx.quit();
///         }
///         std::thread::sleep(Duration::from_millis(1));
///     })
///     .run_headless(5000, |_| {});
/// assert_eq!(world.resource::<Vec<u8>>(), b"hi");
/// host.join().unwrap();
/// ```
#[derive(Default)]
pub struct Networking {
    /// The deterministic lockstep session: this player's input out,
    /// everyone's in. Offline (one player) unless the game was given a
    /// session with [`Game::lockstep`](crate::game::Game::lockstep).
    pub lockstep: Net,
    /// The byte-message endpoint, if the game has one
    /// ([`Game::messages`](crate::game::Game::messages), or set it here at any
    /// time, for example from a menu). The engine polls it; systems send and
    /// read [`events`](NetMessages::events). Set it to `None` to leave.
    pub messages: Option<NetMessages>,
    /// Snapshot streams of remote entities, by the id your packets use.
    pub snapshots: NetSnapshots,
    /// Which peer receives which entity, if the game uses interest
    /// management. The game calls [`Interest::update`] on its send tick.
    pub interest: Option<Interest>,
    /// What the engine's last poll of `messages` returned, if it failed.
    message_error: Option<io::Error>,
}

impl Networking {
    /// Why the engine's last poll of [`messages`](Self::messages) failed, or
    /// `None` if it worked (or there is no endpoint). A socket error, or
    /// `WouldBlock` when thousands of events were left unread: drain
    /// [`NetMessages::events`] every frame. The next poll that works clears it.
    ///
    /// ```
    /// # use ponko::net::*;
    /// # use ponko::prelude::*;
    /// let world = Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     assert!(ctx.net.message_error().is_none());
    /// });
    /// # let _ = world;
    /// ```
    #[must_use]
    pub fn message_error(&self) -> Option<&io::Error> {
        self.message_error.as_ref()
    }

    /// [`Net::player`] of the lockstep session: which player this is.
    ///
    /// The lockstep calls a simulation makes every tick are also methods
    /// here, so `ctx.net.set_input(..)` is `ctx.net.lockstep.set_input(..)`.
    ///
    /// ```
    /// # use ponko::net::*;
    /// # use ponko::prelude::*;
    /// let world = Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     // Reading and writing in one call works, as it does on `Net`.
    ///     ctx.net.set_input(&(ctx.net.player(), ctx.net.players()));
    ///     assert_eq!(ctx.net.state(), NetState::Offline);
    /// });
    /// # let _ = world;
    /// ```
    #[must_use]
    pub fn player(&self) -> usize {
        self.lockstep.player()
    }
    /// [`Net::players`] of the lockstep session.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     assert_eq!(ctx.net.players(), 1); // offline: just this player
    /// });
    /// ```
    #[must_use]
    pub fn players(&self) -> usize {
        self.lockstep.players()
    }
    /// [`Net::state`] of the lockstep session.
    ///
    /// ```
    /// # use ponko::net::NetState;
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     assert_eq!(ctx.net.state(), NetState::Offline);
    /// });
    /// ```
    #[must_use]
    pub fn state(&self) -> NetState {
        self.lockstep.state()
    }
    /// [`Net::error`] of the lockstep session.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     assert!(ctx.net.error().is_none());
    /// });
    /// ```
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.lockstep.error()
    }
    /// [`Net::stats`] of the lockstep session.
    #[must_use]
    pub fn stats(&self) -> NetStats {
        self.lockstep.stats()
    }
    /// [`Net::desync`] of the lockstep session.
    #[must_use]
    pub fn desync(&self) -> Option<&Desync> {
        self.lockstep.desync()
    }
    /// [`Net::local_addr`] of the lockstep session.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.lockstep.local_addr()
    }
    /// [`Net::set_input`] on the lockstep session.
    ///
    /// # Panics
    /// If the encoded input is over [`MAX_INPUT_BYTES`](crate::net::MAX_INPUT_BYTES).
    pub fn set_input<T: Serialize>(&mut self, input: &T) {
        self.lockstep.set_input(input);
    }
    /// [`Net::set_input_bytes`] on the lockstep session.
    ///
    /// # Panics
    /// If `input` is over [`MAX_INPUT_BYTES`](crate::net::MAX_INPUT_BYTES).
    pub fn set_input_bytes(&mut self, input: &[u8]) {
        self.lockstep.set_input_bytes(input);
    }
    /// [`Net::input`] of the lockstep session.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     // No such player: no input.
    ///     assert_eq!(ctx.net.input::<u8>(5), None);
    /// });
    /// ```
    #[must_use]
    pub fn input<T: DeserializeOwned>(&self, player: usize) -> Option<T> {
        self.lockstep.input(player)
    }
    /// [`Net::input_bytes`] of the lockstep session.
    #[must_use]
    pub fn input_bytes(&self, player: usize) -> &[u8] {
        self.lockstep.input_bytes(player)
    }

    /// Once per frame, before any system: polls the message endpoint.
    pub(crate) fn begin_frame(&mut self) {
        let Some(messages) = &mut self.messages else {
            self.message_error = None;
            return;
        };
        self.message_error = messages.poll().err();
    }
}

impl std::fmt::Debug for Networking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Networking")
            .field("lockstep", &self.lockstep)
            .field("messages", &self.messages.is_some())
            .field("snapshots", &self.snapshots.len())
            .field("interest", &self.interest.is_some())
            .finish_non_exhaustive()
    }
}

impl Deref for Networking {
    type Target = Net;
    fn deref(&self) -> &Net {
        &self.lockstep
    }
}
impl DerefMut for Networking {
    fn deref_mut(&mut self) -> &mut Net {
        &mut self.lockstep
    }
}
impl From<Net> for Networking {
    /// A lockstep session and nothing else, as [`Game::lockstep`](crate::game::Game::lockstep) makes.
    fn from(lockstep: Net) -> Self {
        Self {
            lockstep,
            ..Self::default()
        }
    }
}

/// The incoming [`Pose`] snapshots of every remote entity, keyed by the id
/// your packets carry: `ctx.net.snapshots`. One [`SnapshotStream`] per
/// entity plus a [`LatestWins`] filter for duplicate and late packets, so a
/// message handler is `accept`, `stream(id).push`, and drawing is `sample`.
/// The wire format stays yours.
///
/// ```
/// use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
/// let mut snapshots = NetSnapshots::default();
/// // From a decoded packet: entity 7, its stamp, the sender's clock, yours.
/// for n in 0u16..10 {
///     let (sent, arrived) = (500.0 + f64::from(n) * 0.05, f64::from(n) * 0.05 + 0.04);
///     if snapshots.accept(7, UpdateStamp { epoch: 0, seq: n }) {
///         snapshots.stream(7).push(sent, arrived, Pose::at(vec3(f32::from(n), 0.0, 0.0)));
///     }
/// }
/// assert!(!snapshots.accept(7, UpdateStamp { epoch: 0, seq: 3 })); // late
/// // Every frame, with the same clock (`ctx.time.elapsed()` in a game):
/// let shown = snapshots.sample(7, 0.44).unwrap();
/// assert!((shown.value.position.x - 6.0).abs() < 0.01);
/// assert!(snapshots.sample(8, 0.44).is_none());
/// snapshots.remove(7); // the entity despawned
/// assert!(snapshots.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct NetSnapshots {
    streams: HashMap<u64, SnapshotStream<Pose>>,
    newest: LatestWins<u64>,
}

impl NetSnapshots {
    /// Whether an update for `id` with this stamp is newer than every one
    /// accepted before (see [`LatestWins::accept`]): false for duplicates
    /// and packets that arrive late.
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// assert!(snapshots.accept(1, UpdateStamp { epoch: 0, seq: 5 }));
    /// assert!(!snapshots.accept(1, UpdateStamp { epoch: 0, seq: 5 }));
    /// ```
    pub fn accept(&mut self, id: u64, stamp: UpdateStamp) -> bool {
        self.newest.accept(id, stamp)
    }

    /// The stream of `id`, made on first use: push each accepted update
    /// into it with [`SnapshotStream::push`].
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// snapshots.stream(1).push(0.0, 0.0, Pose::default());
    /// assert_eq!(snapshots.len(), 1);
    /// ```
    pub fn stream(&mut self, id: u64) -> &mut SnapshotStream<Pose> {
        self.streams.entry(id).or_default()
    }

    /// The stream of `id`, if it has one.
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// assert!(NetSnapshots::default().get(1).is_none());
    /// ```
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&SnapshotStream<Pose>> {
        self.streams.get(&id)
    }

    /// Where to show `id` at local time `now` ([`SnapshotStream::sample`]);
    /// `None` if nothing arrived for it yet.
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// snapshots.stream(1).push(0.0, 0.0, Pose::at(vec3(2.0, 0.0, 0.0)));
    /// assert_eq!(snapshots.sample(1, 1.0).unwrap().value.position.x, 2.0);
    /// ```
    #[must_use]
    pub fn sample(&self, id: u64, now: f64) -> Option<SnapshotSample<Pose>> {
        self.streams.get(&id)?.sample(now)
    }

    /// Forgets `id`: its stream and its newest stamp. True if it had a stream.
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// snapshots.stream(1);
    /// assert!(snapshots.remove(1) && !snapshots.remove(1));
    /// ```
    pub fn remove(&mut self, id: u64) -> bool {
        self.newest.forget(&id);
        self.streams.remove(&id).is_some()
    }

    /// Forgets every entity (leaving a session).
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// snapshots.stream(1);
    /// snapshots.clear();
    /// assert!(snapshots.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.newest.clear();
        self.streams.clear();
    }

    /// The ids that have a stream, in no particular order.
    ///
    /// ```
    /// # use ponko::{net::{NetSnapshots, Pose, UpdateStamp}, prelude::*};
    /// let mut snapshots = NetSnapshots::default();
    /// snapshots.stream(4);
    /// assert_eq!(snapshots.ids().collect::<Vec<_>>(), [4]);
    /// ```
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.streams.keys().copied()
    }

    /// How many entities have a stream.
    ///
    /// ```
    /// assert_eq!(ponko::net::NetSnapshots::default().len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.streams.len()
    }

    /// Whether no entity has a stream.
    ///
    /// ```
    /// assert!(ponko::net::NetSnapshots::default().is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }
}
