//! Rollback for a whole [`Game`](crate::game::Game): the engine saves and
//! loads the world, the physics, the events and the clock, and runs the
//! simulation systems again from a saved tick.

use std::any::Any;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Advance, Rollback, RollbackDesync, RollbackGame, RollbackStats};
use crate::events::EventsState;
use ponko_ecs::WorldSnapshot;

/// A rollback session for a whole game: [`Rollback`] with the engine as the
/// [`RollbackGame`]. Start it with
/// [`ctx.net.start_rollback`](crate::net::Networking::start_rollback); from
/// then on every simulation tick goes through the session:
///
/// - Frame systems give this player's input with
///   [`set_input`](Self::set_input) and carry the session's
///   [`packet`](Self::packet)s between the peers
///   ([`receive`](Self::receive)), on any transport.
/// - Simulation systems read every player's input for the tick with
///   [`ctx.net.rollback_inputs`](crate::net::Networking::rollback_inputs).
/// - When an input arrives that differs from the prediction, the engine
///   loads the state saved before that tick (the world's components and
///   entity ids, both physics worlds, the events, the tick) and runs the
///   simulation systems again up to the present, in one frame.
///
/// While a tick runs again,
/// [`ctx.net.is_resimulating`](crate::net::Networking::is_resimulating) is
/// true and sounds played are dropped (they were heard the first time).
/// Events sent in a tick that runs again reach simulation systems as they
/// did the first time; frame systems keep their read positions, so they
/// aren't shown events again. Checksums for desync detection are
/// [`world_hash`](crate::reflect::world_hash)es of the saved worlds.
///
/// ```
/// use ponko::net::GameRollback;
/// use ponko::prelude::*;
///
/// // Two peers in one process, packets handed over directly.
/// let mut a = GameRollback::<i8>::new(2, 0);
/// let mut b = GameRollback::<i8>::new(2, 1);
/// a.set_input(1);
/// b.set_input(-1);
/// assert!(b.receive(a.packet()));
/// assert_eq!(a.frame(), 0);
/// ```
///
/// Every component of the world must be `Clone` (the derive notices) or
/// registered with [`World::register_clone`](ponko_ecs::World::register_clone);
/// a tick panics with the component's name otherwise. Resources are saved
/// only if registered with
/// [`World::snapshot_resource`](ponko_ecs::World::snapshot_resource).
pub struct GameRollback<I> {
    session: Rollback<I, GameState>,
    input: I,
    /// The game's tick when the session ran its first tick.
    start: Option<u64>,
}

impl<I> std::fmt::Debug for GameRollback<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameRollback")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

/// What the engine saves before each tick of a [`GameRollback`].
#[derive(Default)]
pub(crate) struct GameState {
    /// The simulation tick this is the state before.
    pub(crate) tick: u64,
    pub(crate) world: WorldSnapshot,
    pub(crate) events: EventsState,
    #[cfg(feature = "physics")]
    pub(crate) physics: crate::physics::PhysicsState,
    #[cfg(feature = "physics3d")]
    pub(crate) physics3d: crate::physics3d::Physics3DState,
    /// The world's hash, on the ticks checksums compare.
    pub(crate) hash: Option<u64>,
    /// How many change ticks before the save each simulation system last
    /// finished (`None`: not yet in this world), so that a tick run again
    /// shows its change filters what they saw the first time.
    pub(crate) seen: Vec<Option<u64>>,
}

impl GameState {
    /// Heap bytes held (the world, the physics and the events), for the
    /// guide's numbers.
    #[allow(dead_code)]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.world.heap_bytes() + self.events.heap_bytes()
    }
}

impl<I> GameRollback<I>
where
    I: Clone + PartialEq + Default + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    /// A session of `players` players (2–8) where this peer is `player`:
    /// [`Rollback::new`].
    ///
    /// # Panics
    ///
    /// Unless `players` is 2 to 8 and `player` below it.
    #[must_use]
    pub fn new(players: usize, player: usize) -> Self {
        Self {
            session: Rollback::new(players, player),
            input: I::default(),
            start: None,
        }
    }

    /// [`Rollback::input_delay`]: input given at tick `t` is used at tick
    /// `t + ticks` everywhere (default 2, at most 16).
    ///
    /// # Panics
    ///
    /// If `ticks` is over 16.
    #[must_use]
    pub fn input_delay(mut self, ticks: u32) -> Self {
        self.session = self.session.input_delay(ticks);
        self
    }

    /// [`Rollback::max_prediction`]: the deepest rollback, and how far the
    /// game runs ahead of the last tick every input is in (default 8, 1 to
    /// 32). The engine keeps this many saved states plus two.
    ///
    /// # Panics
    ///
    /// Unless `ticks` is 1 to 32.
    #[must_use]
    pub fn max_prediction(mut self, ticks: u32) -> Self {
        self.session = self.session.max_prediction(ticks);
        self
    }

    /// [`Rollback::checksum_every`]: compares world hashes with the other
    /// peers every this many ticks (default 60; 0 never). Hashing reads
    /// every reflected component, so it costs a little on those ticks.
    #[must_use]
    pub fn checksum_every(mut self, ticks: u32) -> Self {
        self.session = self.session.checksum_every(ticks);
        self
    }

    /// This player's input for the next tick that runs. It stays until
    /// changed, so set it every frame or only when it changes.
    pub fn set_input(&mut self, input: I) {
        self.input = input;
    }

    /// The input [`set_input`](Self::set_input) gave last.
    #[must_use]
    pub fn input(&self) -> &I {
        &self.input
    }

    /// The packet to send to every other peer, every frame:
    /// [`Rollback::packet`].
    ///
    /// # Panics
    ///
    /// If an input encodes to more than
    /// [`MAX_INPUT_BYTES`](crate::net::MAX_INPUT_BYTES).
    pub fn packet(&mut self) -> &[u8] {
        self.session.packet()
    }

    /// Takes a packet from another peer: [`Rollback::receive`]. A rollback
    /// it calls for runs at the next tick.
    pub fn receive(&mut self, packet: &[u8]) -> bool {
        self.session.receive(packet)
    }

    /// Which player this peer is.
    #[must_use]
    pub fn player(&self) -> usize {
        self.session.player()
    }

    /// How many players there are.
    #[must_use]
    pub fn players(&self) -> usize {
        self.session.players()
    }

    /// Ticks the session ran: the next one it runs. Counted from when it
    /// started, not from the game's first tick.
    #[must_use]
    pub fn frame(&self) -> u64 {
        self.session.frame()
    }

    /// [`Rollback::confirmed_frame`].
    #[must_use]
    pub fn confirmed_frame(&self) -> Option<u64> {
        self.session.confirmed_frame()
    }

    /// [`Rollback::desync`].
    #[must_use]
    pub fn desync(&self) -> Option<RollbackDesync> {
        self.session.desync()
    }

    /// [`Rollback::stats`].
    #[must_use]
    pub fn stats(&self) -> RollbackStats {
        self.session.stats()
    }
}

/// The engine side of a tick of a [`GameRollback`]: the game loop.
pub(crate) trait Runner {
    /// The game's simulation tick: the next one to run.
    fn tick(&self) -> u64;
    /// Saves the state before the next tick (not its hash).
    fn save(&mut self, into: &mut GameState);
    fn load(&mut self, from: &GameState);
    /// The world's hash, for checksums.
    fn hash(&self) -> u64;
    /// Where simulation systems read the inputs: a `Vec<I>`.
    fn inputs(&mut self) -> &mut (dyn Any + Send + Sync);
    /// Runs one tick with the inputs put there.
    fn run_tick(&mut self);
}

/// A [`GameRollback`] without its input type, as `ctx.net` keeps it.
pub(crate) trait Session: Send + Sync {
    fn advance(&mut self, runner: &mut dyn Runner) -> Advance;
    /// Extra ticks events must stay: as far back as a load goes.
    fn hold_ticks(&self) -> u32;
    fn new_inputs(&self) -> Box<dyn Any + Send + Sync>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<I> Session for GameRollback<I>
where
    I: Clone + PartialEq + Default + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn advance(&mut self, runner: &mut dyn Runner) -> Advance {
        let start = *self.start.get_or_insert(runner.tick());
        let mut game = Adapter::<I> {
            runner,
            start,
            hash_every: u64::from(self.session.checksum_every_ticks()),
            _input: std::marker::PhantomData,
        };
        self.session.advance(&mut game, self.input.clone())
    }

    fn hold_ticks(&self) -> u32 {
        self.session.max_prediction_ticks() + 3
    }

    fn new_inputs(&self) -> Box<dyn Any + Send + Sync> {
        Box::new(vec![I::default(); self.session.players()])
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// The game loop as a [`RollbackGame`].
struct Adapter<'r, I> {
    runner: &'r mut dyn Runner,
    /// The game's tick at the session's frame 0.
    start: u64,
    /// The session compares the states before its frames 0, `hash_every`,
    /// `2 × hash_every`, ...: only those are hashed.
    hash_every: u64,
    _input: std::marker::PhantomData<fn(I)>,
}

impl<I: Clone + Send + Sync + 'static> RollbackGame<I> for Adapter<'_, I> {
    type State = GameState;

    fn save(&mut self, into: &mut GameState) {
        self.runner.save(into);
        let frame = into.tick.wrapping_sub(self.start);
        into.hash = (self.hash_every > 0 && frame.is_multiple_of(self.hash_every)).then(|| self.runner.hash());
    }

    fn load(&mut self, from: &GameState) {
        self.runner.load(from);
    }

    fn advance(&mut self, inputs: &[I]) {
        let slot = self
            .runner
            .inputs()
            .downcast_mut::<Vec<I>>()
            .expect("rollback inputs type");
        slot.clone_from_slice(inputs);
        self.runner.run_tick();
    }

    fn checksum(state: &GameState) -> Option<u64> {
        state.hash
    }
}
