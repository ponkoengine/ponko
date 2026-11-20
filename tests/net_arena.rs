//! The `net_arena` reference game, played with no window: a dedicated
//! server and two bots, each a whole game on its own thread, over an
//! in-memory network that loses every 11th datagram and delivers the rest
//! newest first. The sides take turns, a whole frame each, always in the same
//! order, and the endpoints count game time, so a run is the same every time
//! and nothing waits on the wall clock.

#[path = "../examples/net_arena/game.rs"]
#[allow(dead_code)]
mod game;

use game::*;
use ponko::net::{ClientStats, NetEvent, NetId, NetMessages, Replica, Replicated};
use ponko::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Ticks in a run: 15 seconds of play.
const TICKS: u64 = 900;
/// The bots stop this many ticks before the end, so everything settles.
const SETTLE: u64 = 120;

/// Takes turns: one side plays a whole frame (receives, ticks, sends),
/// then the next. Threads, but one at a time and always in the same order.
struct Turns {
    turn: Mutex<usize>,
    changed: Condvar,
    sides: usize,
}

impl Turns {
    fn new(sides: usize) -> Arc<Self> {
        Arc::new(Self {
            turn: Mutex::new(0),
            changed: Condvar::new(),
            sides,
        })
    }

    fn wait(&self, me: usize) {
        let mut turn = self.turn.lock().unwrap();
        while *turn != me {
            let (next, waited) = self.changed.wait_timeout(turn, Duration::from_secs(60)).unwrap();
            turn = next;
            assert!(
                !waited.timed_out() || *turn == me,
                "another side stopped (did it panic?)"
            );
        }
    }

    fn pass(&self, me: usize) {
        *self.turn.lock().unwrap() = (me + 1) % self.sides;
        self.changed.notify_all();
    }
}

// ANCHOR: take_turns
/// Runs `game` as side `me`: its frames interleave with the other sides'.
fn take_turns(game: Game, turns: Arc<Turns>, me: usize) -> World {
    let after = turns.clone();
    game.headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            after.pass(me);
            if ctx.time.sim_tick() < TICKS {
                after.wait(me);
            }
        })
        .run_headless(TICKS, move |_| turns.wait(me))
}
// ANCHOR_END: take_turns

/// What a bot saw.
#[derive(Default)]
struct Seen {
    collected: Vec<Collected>,
    waved: Vec<NetId>,
    stats: ClientStats,
    joined_at: Option<u64>,
    waved_once: bool,
}

/// A bot client: plays until `SETTLE` ticks before the end, waves once.
fn client(net: NetMessages) -> Game {
    build("bot")
        .messages(net)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() < TICKS - SETTLE {
                bot(ctx);
            } else {
                ctx.net.replication.set_input(Controls::default());
            }
        })
        .system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let mut seen = ctx.world.remove_resource::<Seen>().unwrap_or_default();
            if ctx.net.replication.is_joined() && seen.joined_at.is_none() {
                seen.joined_at = Some(tick);
            }
            // Wave once, as soon as this bot's player shows up.
            let mut mine = false;
            ctx.world
                .query::<(&Player, &Replica)>()
                .for_each(|(_, r)| mine |= r.is_mine());
            if mine && !seen.waved_once {
                seen.waved_once = true;
                ctx.net.replication.to_server(&Wave);
            }
            seen.collected
                .extend(ctx.events.read::<NetEvent<Collected>>().iter().map(|c| c.event.clone()));
            seen.waved
                .extend(ctx.events.read::<NetEvent<Waved>>().iter().map(|w| w.event.by));
            seen.stats = ctx.net.replication.client_stats();
            ctx.world.insert_resource(seen);
        })
}

/// The players and their scores, by net id.
fn players(world: &mut World) -> BTreeMap<u32, (Player, u16)> {
    let mut out = BTreeMap::new();
    world
        .query::<(&Player, &Score, Option<&Replica>, Option<&Replicated>)>()
        .for_each(|(p, s, replica, original)| {
            let id = replica
                .map(Replica::id)
                .or_else(|| original.and_then(Replicated::net_id));
            out.insert(id.expect("a replicated player").index(), (*p, s.0));
        });
    out
}

// ANCHOR: play
/// One run: the server's world and each bot's.
fn play() -> (World, Vec<World>) {
    // On the game's clock: resends and timeouts count ticks, not the
    // (much shorter) wall time a headless run takes. The loss is periodic,
    // so its period must not divide the resend time (100 ms, 7 frames
    // here): with every 7th datagram lost, one resent message would be
    // lost every time.
    let mut nets: Vec<NetMessages> = NetMessages::loopback_lossy(3, 11)
        .into_iter()
        .map(NetMessages::game_clock)
        .collect();
    let host = nets.remove(0);
    let turns = Turns::new(3);
    let bots: Vec<_> = nets
        .into_iter()
        .enumerate()
        .map(|(n, net)| {
            let turns = turns.clone();
            std::thread::spawn(move || take_turns(client(net), turns, n + 1))
        })
        .collect();
    let server = take_turns(build("server").messages(host), turns, 0);
    let bots = bots.into_iter().map(|b| b.join().expect("a bot panicked")).collect();
    (server, bots)
}
// ANCHOR_END: play

#[test]
fn bots_join_a_dedicated_server_play_and_see_the_same_arena() {
    let (mut server, mut bots) = play();

    // The server made one player per bot and owns the scores.
    let truth = players(&mut server);
    assert_eq!(truth.len(), 2, "one player per bot: {truth:?}");
    let total: u32 = truth.values().map(|(_, s)| u32::from(*s)).sum();
    assert!(total >= 4, "the bots collect orbs: {truth:?}");
    let mut orbs = 0;
    server.query::<&Orb>().for_each(|_| orbs += 1);
    assert_eq!(orbs, ORBS, "a collected orb comes back");

    let mut collected = 0;
    for bot in &mut bots {
        let seen = bot.remove_resource::<Seen>().expect("the bot ran");
        assert!(
            seen.joined_at.is_some_and(|t| t < 60),
            "joined within a second: {:?}",
            seen.joined_at
        );
        // Once settled, every bot sees both players exactly where the server
        // has them, with the same scores.
        assert_eq!(players(bot), truth, "a bot's view of the players");
        // Its own player is predicted, the other one isn't.
        let mut mine = 0;
        bot.query::<(&Player, &Replica)>()
            .for_each(|(_, r)| mine += usize::from(r.is_mine()));
        assert_eq!(mine, 1, "one player is mine");
        // Prediction agreed with the server: the step and the controls are
        // quantized the same on both sides.
        assert!(seen.stats.confirmed > 100, "inputs were confirmed: {:?}", seen.stats);
        assert_eq!(
            seen.stats.corrections, 0,
            "predictions were never wrong: {:?}",
            seen.stats
        );
        // Every orb is near enough to replicate (the far ring covers the arena).
        let mut seen_orbs = 0;
        bot.query::<(&Orb, &Replica)>().for_each(|_| seen_orbs += 1);
        assert_eq!(seen_orbs, ORBS);
        // Events: every collection reached every bot once, in order, and
        // both waves came back.
        let mut scores: BTreeMap<u32, u16> = BTreeMap::new();
        for c in &seen.collected {
            let last = scores.entry(c.by.index()).or_default();
            assert_eq!(c.score, *last + 1, "collections arrive once and in order");
            *last = c.score;
        }
        let expected: BTreeMap<u32, u16> = truth
            .iter()
            .map(|(id, (_, s))| (*id, *s))
            .filter(|(_, s)| *s > 0)
            .collect();
        assert_eq!(scores, expected, "a bot heard of every collection");
        collected += seen.collected.len();
        let mut waved = seen.waved.clone();
        waved.sort_by_key(|id| id.index());
        let ids: Vec<NetId> = truth.keys().map(|&i| NetId::from_index(i)).collect();
        assert_eq!(waved, ids, "both bots waved and everyone heard");
    }
    assert_eq!(collected, 2 * total as usize);
}

#[test]
fn the_arena_plays_the_same_every_time() {
    let (mut a, _) = play();
    let (mut b, _) = play();
    assert_eq!(players(&mut a), players(&mut b));
}
