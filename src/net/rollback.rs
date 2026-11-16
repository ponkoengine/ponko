//! Rollback netcode (GGPO-style) for small deterministic games: every peer
//! runs the game at once with its own input, **predicts** the inputs it
//! hasn't received yet (the player's last one), and when a real input
//! arrives that differs, **rolls back**: loads the state saved before that
//! tick and simulates the ticks since again with the real inputs.
//!
//! [`Rollback`] is the session: input delay, prediction, the saved states,
//! when to roll back, and the packets that carry inputs between peers (any
//! transport: [`NetMessages`](crate::net::NetMessages) on an unreliable lane,
//! your own sockets, a test's scripted link). The game is a
//! [`RollbackGame`]: it saves and loads its own state and advances one tick.
//! What that state holds is the game's choice, so a fighting game with two
//! characters saves a few hundred bytes, not a world.
//!
//! What a game must capture: the tick (it seeds [`Ctx::random`](crate::ctx::Ctx::random),
//! which keeps no other state), the components and resources a tick
//! changes, any `Rng` it keeps, events read a tick after they were sent,
//! and its physics. Sounds and effects are output: play them for confirmed
//! ticks only. [`World::load`](crate::reflect::WorldReflect::load) is not a
//! rollback snapshot: it costs far more than copying components and resets
//! the entity allocator, so later spawns can get other ids than on a peer
//! that didn't roll back. The guide's "Rollback" section has the numbers.

use std::collections::VecDeque;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Ticks of input the session remembers per player. Peers are never more
/// than `2 × (input_delay + max_prediction) + 1` ticks apart in what they
/// still need, far less than this.
const RING: usize = 256;
/// The most ticks [`Rollback::input_delay`] takes.
const MAX_DELAY: u32 = 16;
/// The most ticks [`Rollback::max_prediction`] takes.
const MAX_PREDICTION: u32 = 32;
/// Own checksums kept to compare with late ones from peers.
const CHECKS_KEPT: usize = 32;
/// Packets are at most this long.
const PACKET: usize = 1400;

const MAGIC: [u8; 4] = *b"PKRB";
const VERSION: u8 = 1;
/// Magic, version, sender, players, delay, then one acknowledgement per
/// player, the checksum (frame, value, flag) and the input range (first, count).
const HEADER: usize = 4 + 1 + 1 + 1 + 1 + 8 * crate::net::MAX_PLAYERS + 8 + 8 + 1 + 8 + 2;

/// A game that can be rolled back: it saves and loads its state and
/// advances one tick from every player's input. [`Rollback::advance`]
/// calls these.
///
/// Advancing must be **deterministic**: the same state and inputs give the
/// same next state on every peer (the same build, no wall clock, no
/// unseeded randomness, no iteration over hash maps). Everything a tick
/// reads that a later tick can change belongs in the state.
///
/// ```
/// use ponko::net::RollbackGame;
///
/// #[derive(Clone, Default)]
/// struct Duel { x: [i32; 2] }
///
/// impl RollbackGame<i8> for Duel {
///     type State = Duel;
///     fn save(&mut self, into: &mut Duel) { into.clone_from(self); }
///     fn load(&mut self, from: &Duel) { self.clone_from(from); }
///     fn advance(&mut self, inputs: &[i8]) {
///         for (x, dx) in self.x.iter_mut().zip(inputs) { *x += i32::from(*dx); }
///     }
///     fn checksum(state: &Duel) -> Option<u64> {
///         Some(((state.x[0] as u64) << 32) | u64::from(state.x[1] as u32))
///     }
/// }
/// ```
pub trait RollbackGame<I> {
    /// A saved state. The session keeps `max_prediction + 2` of them and
    /// writes over the oldest, so saving reuses their memory.
    type State: Default;
    /// Copies the current state into `into` (a state saved before, or a
    /// default one the first times).
    fn save(&mut self, into: &mut Self::State);
    /// Makes `from` the current state.
    fn load(&mut self, from: &Self::State);
    /// Runs one tick with every player's input, indexed by player: real
    /// ones where they arrived, predicted ones where not.
    fn advance(&mut self, inputs: &[I]);
    /// A hash of a saved state for desync detection
    /// ([`Rollback::checksum_every`]); `None` (the default) checks nothing.
    fn checksum(state: &Self::State) -> Option<u64> {
        let _ = state;
        None
    }
}

/// What one [`Rollback::advance`] did.
///
/// ```
/// let a = ponko::net::Advance::Ran { resimulated: 0 };
/// assert!(a.ran());
/// assert!(!ponko::net::Advance::Stalled.ran());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Advance {
    /// The tick ran, after simulating `resimulated` earlier ticks again
    /// because inputs that arrived differed from the prediction (0: no
    /// rollback).
    Ran {
        /// Ticks simulated again before this one.
        resimulated: u32,
    },
    /// The tick didn't run: a peer's inputs are
    /// [`max_prediction`](Rollback::max_prediction) ticks behind. Call
    /// again next tick (a rollback may still have run).
    Stalled,
}

impl Advance {
    /// Whether the tick ran.
    #[must_use]
    pub fn ran(self) -> bool {
        matches!(self, Self::Ran { .. })
    }
}

/// Counters of a [`Rollback`] session ([`Rollback::stats`]).
///
/// ```
/// let rb = ponko::net::Rollback::<u8, ()>::new(2, 0);
/// assert_eq!(rb.stats(), ponko::net::RollbackStats::default());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RollbackStats {
    /// Ticks that ran (not counting the ones simulated again).
    pub ticks: u64,
    /// Ticks that ran with at least one predicted input.
    pub predicted: u64,
    /// Rollbacks: real inputs that differed from the prediction.
    pub rollbacks: u64,
    /// Ticks simulated again by rollbacks, in total.
    pub resimulated: u64,
    /// The most ticks one rollback simulated again.
    pub deepest: u32,
    /// Calls to [`advance`](Rollback::advance) that stalled.
    pub stalls: u64,
    /// Packets built with [`packet`](Rollback::packet).
    pub packets_sent: u64,
    /// Bytes in those packets.
    pub bytes_sent: u64,
    /// Packets [`receive`](Rollback::receive) ignored: not a rollback
    /// packet, from a session with other settings, or malformed.
    pub rejected: u64,
}

/// Two peers' states stopped matching ([`Rollback::desync`]).
///
/// ```
/// let d = ponko::net::RollbackDesync { frame: 60, player: 1 };
/// assert_eq!(d.frame, 60);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackDesync {
    /// The first checked tick whose state differed.
    pub frame: u64,
    /// The player whose state differs from ours.
    pub player: usize,
}

/// A rollback session for one peer: GGPO-style netcode for 2–8 players of a
/// deterministic game that is cheap to save and load (fighting, racing,
/// small arena and party games).
///
/// Every tick, give [`advance`](Self::advance) this player's input and the
/// game. It schedules the input [`input_delay`](Self::input_delay) ticks
/// ahead (default 2), runs the tick with the other players' inputs where
/// they are in and their last input where not, and saves the state before
/// it. When [`receive`](Self::receive) brings an input that differs from
/// what was predicted, the next `advance` loads the state of that tick and
/// simulates up to now again. A peer more than
/// [`max_prediction`](Self::max_prediction) ticks (default 8) behind stalls
/// the session until its inputs come.
///
/// Inputs travel in [`packet`](Self::packet)s: send one to every other peer
/// every tick, over any transport, losing or reordering them as networks do.
/// Each packet repeats every input the peer hasn't acknowledged, so a lost
/// one costs nothing but bytes. All peers need the same settings (players,
/// input delay); packets from a session with others are ignored.
///
/// `I` is one player's input for one tick (encoded in packets with serde as
/// JSON, at most [`MAX_INPUT_BYTES`](crate::net::MAX_INPUT_BYTES)); its
/// `Default` is "no input", used for the first `input_delay` ticks. `S` is
/// the game's saved state ([`RollbackGame::State`]).
///
/// ```
/// use ponko::net::{Rollback, RollbackGame};
///
/// // The whole game: each player's position, moved by their input.
/// #[derive(Clone, Default, PartialEq, Debug)]
/// struct Duel { x: [i32; 2] }
/// impl RollbackGame<i8> for Duel {
///     type State = Duel;
///     fn save(&mut self, into: &mut Duel) { into.clone_from(self); }
///     fn load(&mut self, from: &Duel) { self.clone_from(from); }
///     fn advance(&mut self, inputs: &[i8]) {
///         for (x, dx) in self.x.iter_mut().zip(inputs) { *x += i32::from(*dx); }
///     }
/// }
///
/// // No input delay, so every change of input is guessed wrong once.
/// let mut peers = [Rollback::new(2, 0).input_delay(0), Rollback::new(2, 1).input_delay(0)];
/// let mut games = [Duel::default(), Duel::default()];
/// let mut wire: Vec<(usize, Vec<u8>)> = Vec::new(); // (to, packet)
/// for tick in 0..30 {
///     for p in 0..2 {
///         let input = if p == 0 { 1 } else { (tick % 3) as i8 - 1 };
///         peers[p].advance(&mut games[p], input);
///         wire.push((1 - p, peers[p].packet().to_vec()));
///     }
///     // Packets arrive one tick late.
///     for (to, bytes) in std::mem::take(&mut wire) {
///         peers[to].receive(&bytes);
///     }
/// }
/// assert_eq!(peers[0].confirmed_frame(), Some(29)); // every input of the 30 ticks is in
/// // Each peer guessed the other's input wrong and rolled back.
/// assert!(peers[0].stats().rollbacks > 0 && peers[1].stats().rollbacks > 0);
/// ```
pub struct Rollback<I, S> {
    player: usize,
    players: usize,
    delay: u32,
    max_prediction: u32,
    checksum_every: u32,
    /// The next tick to run.
    frame: u64,
    /// Per player, by `frame % RING`: the real inputs that arrived.
    inputs: Vec<Vec<I>>,
    /// Per player, by `frame % RING`: the input the last simulation of that
    /// tick used (real or predicted).
    used: Vec<Vec<I>>,
    /// Per player: every input before this tick is in.
    have: Vec<u64>,
    /// Per player: how many of our inputs they have (from their packets).
    acked: Vec<u64>,
    /// The earliest tick that ran with a prediction that turned out wrong.
    wrong: Option<u64>,
    /// By `frame % states.len()`: (tick, the state before it ran).
    states: Vec<(Option<u64>, S)>,
    /// Every input for one tick, reused.
    current: Vec<I>,
    /// `I::default()`: the prediction before a player's first input.
    none: I,
    /// The next tick whose state to checksum.
    next_check: u64,
    /// Our checksums (tick, value), newest last.
    own_checks: VecDeque<(u64, u64)>,
    /// Peers' checksums we couldn't compare yet: (player, tick, value).
    their_checks: VecDeque<(usize, u64, u64)>,
    desync: Option<RollbackDesync>,
    stats: RollbackStats,
    out: Vec<u8>,
    scratch: Vec<u8>,
    /// The inputs of the packet being read, reused.
    incoming: Vec<I>,
}

impl<I, S> std::fmt::Debug for Rollback<I, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rollback")
            .field("player", &self.player)
            .field("players", &self.players)
            .field("frame", &self.frame)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl<I, S> Rollback<I, S>
where
    I: Clone + PartialEq + Default + Serialize + DeserializeOwned,
    S: Default,
{
    /// A session of `players` players (2–8) where this peer is `player`.
    ///
    /// ```
    /// let rb = ponko::net::Rollback::<u8, ()>::new(2, 1);
    /// assert_eq!((rb.player(), rb.players(), rb.frame()), (1, 2, 0));
    /// ```
    ///
    /// # Panics
    ///
    /// Unless `players` is 2 to 8 and `player` below it.
    #[must_use]
    pub fn new(players: usize, player: usize) -> Self {
        assert!(
            (2..=crate::net::MAX_PLAYERS).contains(&players),
            "Rollback: expected 2 to 8 players; pass a player count in that range"
        );
        assert!(
            player < players,
            "Rollback: player {player} of {players}; pass a zero-based player index below the count"
        );
        let mut rb = Self {
            player,
            players,
            delay: 0,
            max_prediction: 8,
            checksum_every: 60,
            frame: 0,
            inputs: vec![vec![I::default(); RING]; players],
            used: vec![vec![I::default(); RING]; players],
            have: vec![0; players],
            acked: vec![0; players],
            wrong: None,
            states: Vec::new(),
            current: vec![I::default(); players],
            none: I::default(),
            next_check: 0,
            own_checks: VecDeque::new(),
            their_checks: VecDeque::new(),
            desync: None,
            stats: RollbackStats::default(),
            out: Vec::with_capacity(PACKET),
            scratch: Vec::new(),
            incoming: Vec::new(),
        };
        rb.set_delay(2);
        rb.resize_states();
        rb
    }

    /// Schedules each input this many ticks ahead (default 2, at most 16):
    /// input given at tick `t` is used at tick `t + delay` everywhere. A
    /// little delay means fewer and shorter rollbacks (a 2-tick delay hides
    /// a 33 ms round trip at 60 Hz entirely); 0 rolls back on every input
    /// that changes. Every peer must use the same.
    ///
    /// # Panics
    ///
    /// If `ticks` is over 16, or the session already ran a tick.
    #[must_use]
    pub fn input_delay(mut self, ticks: u32) -> Self {
        assert!(ticks <= MAX_DELAY, "input delay is at most {MAX_DELAY} ticks");
        assert_eq!(self.frame, 0, "set the input delay before the first tick");
        self.set_delay(ticks);
        self
    }

    /// Runs at most this many ticks past the last tick every input is in
    /// (default 8, 1 to 32), then stalls. It is also the deepest rollback,
    /// so it bounds the work of one tick: `max_prediction + 1` simulated
    /// ticks and saves.
    ///
    /// # Panics
    ///
    /// Unless `ticks` is 1 to 32, or if the session already ran a tick.
    #[must_use]
    pub fn max_prediction(mut self, ticks: u32) -> Self {
        assert!(
            (1..=MAX_PREDICTION).contains(&ticks),
            "max prediction is 1 to {MAX_PREDICTION} ticks"
        );
        assert_eq!(self.frame, 0, "set the prediction window before the first tick");
        self.max_prediction = ticks;
        self.resize_states();
        self
    }

    /// Compares state checksums ([`RollbackGame::checksum`]) with the other
    /// peers every this many confirmed ticks (default 60; 0 never). A
    /// difference is a [`desync`](Self::desync).
    #[must_use]
    pub fn checksum_every(mut self, ticks: u32) -> Self {
        self.checksum_every = ticks;
        self
    }

    fn set_delay(&mut self, ticks: u32) {
        self.delay = ticks;
        // The first `delay` ticks have no input from anyone: the default.
        self.have.fill(u64::from(ticks));
        self.acked.fill(u64::from(ticks));
    }

    fn resize_states(&mut self) {
        self.states.clear();
        self.states
            .resize_with(self.max_prediction as usize + 2, || (None, S::default()));
    }

    /// Which player this peer is.
    #[must_use]
    pub fn player(&self) -> usize {
        self.player
    }
    /// How many players there are.
    #[must_use]
    pub fn players(&self) -> usize {
        self.players
    }
    /// The next tick [`advance`](Self::advance) runs.
    #[must_use]
    pub fn frame(&self) -> u64 {
        self.frame
    }
    /// The last tick every player's input is in for (`None` before the
    /// first): ticks up to it will never be rolled back again.
    #[must_use]
    pub fn confirmed_frame(&self) -> Option<u64> {
        self.have.iter().min().copied().filter(|&h| h > 0).map(|h| h - 1)
    }
    /// The first desync found, if any.
    #[must_use]
    pub fn desync(&self) -> Option<RollbackDesync> {
        self.desync
    }
    /// What the session did so far.
    #[must_use]
    pub fn stats(&self) -> RollbackStats {
        self.stats
    }

    /// Runs one tick: rolls back first if inputs arrived that differ from
    /// the prediction, then schedules `input` (this player's) for tick
    /// `frame + input_delay` and runs `frame`, unless a peer is too far
    /// behind ([`Advance::Stalled`]: give the same input again next tick;
    /// the first one given for a tick counts).
    pub fn advance<G>(&mut self, game: &mut G, input: I) -> Advance
    where
        G: RollbackGame<I, State = S>,
    {
        let me = self.player;
        let due = self.frame + u64::from(self.delay);
        if self.have[me] == due {
            self.inputs[me][slot(due)] = input;
            self.have[me] += 1;
        }
        let mut resimulated = 0;
        if let Some(from) = self.wrong.take()
            && from < self.frame
        {
            let i = self.state_slot(from);
            debug_assert_eq!(self.states[i].0, Some(from), "rollbacks stay within the saved states");
            game.load(&self.states[i].1);
            for tick in from..self.frame {
                if tick != from {
                    self.save(game, tick);
                }
                self.simulate(game, tick);
            }
            resimulated = (self.frame - from) as u32;
            self.stats.rollbacks += 1;
            self.stats.resimulated += u64::from(resimulated);
            self.stats.deepest = self.stats.deepest.max(resimulated);
        }
        let behind = self
            .have
            .iter()
            .enumerate()
            .filter(|&(p, _)| p != me)
            .map(|(_, &h)| h)
            .min()
            .unwrap_or(u64::MAX);
        let advance = if self.frame >= behind + u64::from(self.max_prediction) {
            self.stats.stalls += 1;
            Advance::Stalled
        } else {
            self.save(game, self.frame);
            if self.simulate(game, self.frame) {
                self.stats.predicted += 1;
            }
            self.frame += 1;
            self.stats.ticks += 1;
            Advance::Ran { resimulated }
        };
        self.check::<G>();
        advance
    }

    fn state_slot(&self, tick: u64) -> usize {
        (tick % self.states.len() as u64) as usize
    }

    fn save<G: RollbackGame<I, State = S>>(&mut self, game: &mut G, tick: u64) {
        let i = self.state_slot(tick);
        let slot = &mut self.states[i];
        game.save(&mut slot.1);
        slot.0 = Some(tick);
    }

    /// Runs `tick` with the inputs known now; true if any was predicted.
    fn simulate<G: RollbackGame<I, State = S>>(&mut self, game: &mut G, tick: u64) -> bool {
        let mut predicted = false;
        for p in 0..self.players {
            let input = if tick < self.have[p] {
                &self.inputs[p][slot(tick)]
            } else {
                predicted = true;
                // The player's last input, or none yet.
                match self.have[p].checked_sub(1) {
                    Some(last) => &self.inputs[p][slot(last)],
                    None => &self.none,
                }
            };
            self.current[p].clone_from(input);
            self.used[p][slot(tick)].clone_from(input);
        }
        game.advance(&self.current);
        predicted
    }

    /// Checksums the saved states every input is in for, and compares them.
    fn check<G: RollbackGame<I, State = S>>(&mut self) {
        if self.checksum_every == 0 {
            return;
        }
        let every = u64::from(self.checksum_every);
        let confirmed = self.have.iter().min().copied().unwrap_or(0);
        // A state saved before tick `c` is final once every input before `c`
        // is in (and it ran: `c < frame`).
        while self.next_check <= confirmed && self.next_check < self.frame {
            let c = self.next_check;
            self.next_check += every;
            let i = self.state_slot(c);
            if self.states[i].0 != Some(c) {
                continue; // fell out of the window (a long stall); skip it
            }
            let Some(sum) = G::checksum(&self.states[i].1) else {
                return;
            };
            self.own_checks.push_back((c, sum));
            if self.own_checks.len() > CHECKS_KEPT {
                self.own_checks.pop_front();
            }
            let theirs: Vec<_> = self.their_checks.iter().filter(|t| t.1 == c).copied().collect();
            self.their_checks.retain(|t| t.1 != c);
            for (player, _, value) in theirs {
                self.compare(player, c, value);
            }
        }
    }

    fn compare(&mut self, player: usize, frame: u64, value: u64) {
        let Some(&(_, ours)) = self.own_checks.iter().find(|o| o.0 == frame) else {
            if self.own_checks.back().is_none_or(|o| o.0 < frame)
                && !self.their_checks.iter().any(|t| t.0 == player && t.1 == frame)
            {
                self.their_checks.push_back((player, frame, value));
                if self.their_checks.len() > CHECKS_KEPT {
                    self.their_checks.pop_front();
                }
            }
            return;
        };
        if ours != value && self.desync.is_none_or(|d| d.frame > frame) {
            self.desync = Some(RollbackDesync { frame, player });
        }
    }

    /// The packet to send to every other peer this tick: this player's
    /// inputs the peers haven't acknowledged, what we have of theirs, and
    /// our newest checksum. Send it every tick (also while stalled), on an
    /// unreliable channel; at most 1400 bytes.
    ///
    /// # Panics
    ///
    /// If one of this player's inputs encodes to more than
    /// [`MAX_INPUT_BYTES`](crate::net::MAX_INPUT_BYTES).
    pub fn packet(&mut self) -> &[u8] {
        let me = self.player;
        let mut out = std::mem::take(&mut self.out);
        out.clear();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&[VERSION, me as u8, self.players as u8, self.delay as u8]);
        for p in 0..crate::net::MAX_PLAYERS {
            out.extend_from_slice(&self.have.get(p).copied().unwrap_or(0).to_le_bytes());
        }
        let (frame, sum, flag) = self.own_checks.back().map_or((0, 0, 0), |&(f, s)| (f, s, 1));
        out.extend_from_slice(&frame.to_le_bytes());
        out.extend_from_slice(&sum.to_le_bytes());
        out.push(flag);
        // From the oldest input any peer is missing.
        let first = (0..self.players)
            .filter(|&p| p != me)
            .map(|p| self.acked[p])
            .min()
            .unwrap_or(self.have[me])
            .max(self.have[me].saturating_sub(RING as u64 / 2));
        out.extend_from_slice(&first.to_le_bytes());
        let count_at = out.len();
        out.extend_from_slice(&0u16.to_le_bytes());
        let mut count = 0u16;
        for tick in first..self.have[me] {
            self.scratch.clear();
            serde_json::to_writer(&mut self.scratch, &self.inputs[me][slot(tick)])
                .expect("Rollback: input serialization failed; fix the input type serializer");
            assert!(
                self.scratch.len() <= crate::net::MAX_INPUT_BYTES,
                "input is over {} bytes; send a smaller input type",
                crate::net::MAX_INPUT_BYTES
            );
            if out.len() + 2 + self.scratch.len() > PACKET {
                break; // the rest goes in the next packets
            }
            out.extend_from_slice(&(self.scratch.len() as u16).to_le_bytes());
            out.extend_from_slice(&self.scratch);
            count += 1;
        }
        out[count_at..count_at + 2].copy_from_slice(&count.to_le_bytes());
        debug_assert!(HEADER <= out.len() && out.len() <= PACKET);
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += out.len() as u64;
        self.out = out;
        &self.out
    }

    /// Takes a packet from another peer's [`packet`](Self::packet). Returns
    /// false (and counts it in [`RollbackStats::rejected`]) for anything
    /// else: not a rollback packet, from this player, from a session with
    /// other settings, or cut short. Duplicates and old packets are fine.
    pub fn receive(&mut self, packet: &[u8]) -> bool {
        let ok = self.read(packet).is_some();
        if !ok {
            self.stats.rejected += 1;
        }
        ok
    }

    fn read(&mut self, packet: &[u8]) -> Option<()> {
        let mut r = Reader(packet);
        if r.take(4)? != MAGIC || r.u8()? != VERSION {
            return None;
        }
        let from = usize::from(r.u8()?);
        let players = usize::from(r.u8()?);
        let delay = u32::from(r.u8()?);
        if from == self.player || from >= self.players || players != self.players || delay != self.delay {
            return None;
        }
        let mut acks = [0u64; crate::net::MAX_PLAYERS];
        for a in &mut acks {
            *a = r.u64()?;
        }
        let (check_frame, check_sum, check_flag) = (r.u64()?, r.u64()?, r.u8()?);
        let first = r.u64()?;
        let count = r.u16()?;
        // Parse everything before changing anything.
        let mut inputs = std::mem::take(&mut self.incoming);
        inputs.clear();
        let parsed = (0..count).try_for_each(|_| {
            let len = usize::from(r.u16()?);
            inputs.push(serde_json::from_slice::<I>(r.take(len)?).ok()?);
            Some(())
        });
        if parsed.is_none() || !r.0.is_empty() {
            self.incoming = inputs;
            return None;
        }
        // What they have of ours (acknowledgements only grow; a stale
        // packet can't take one back).
        self.acked[from] = self.acked[from].max(acks[self.player].min(self.have[self.player]));
        for (tick, input) in (first..).zip(inputs.drain(..)) {
            if tick != self.have[from] {
                continue; // a repeat, or past a gap (inputs come in order)
            }
            if tick < self.frame && self.used[from][slot(tick)] != input {
                self.wrong = Some(self.wrong.map_or(tick, |w| w.min(tick)));
            }
            self.inputs[from][slot(tick)] = input;
            self.have[from] += 1;
        }
        self.incoming = inputs;
        if check_flag == 1 {
            self.compare(from, check_frame, check_sum);
        }
        Some(())
    }
}

fn slot(tick: u64) -> usize {
    (tick % RING as u64) as usize
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes")))
    }
}
