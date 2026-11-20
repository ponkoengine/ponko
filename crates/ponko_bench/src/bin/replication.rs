//! Declared replication at scale: a dedicated server replicating 1000
//! entities to 16 clients over the in-memory loopback. Every side is a
//! whole headless game on its own thread; they take turns frame by frame
//! and count game time (`NetMessages::game_clock`), so the bytes are the
//! same on every run and every machine. Only the CPU time varies.
//!
//! Reports, per scenario, after a 2 second warm-up (the spawn burst):
//! - bytes per second each client receives: replication's packets alone
//!   (`PeerStats::bytes_sent`) and on the wire (`MessageStats`, with the
//!   transport's framing, acks and resends);
//! - the server's CPU time per tick: its whole frame (receiving, the
//!   game's systems, extracting changes, encoding and sending to every
//!   client), median and 90th percentile over the ticks of a run, and the
//!   spread of the median over the runs.
//!
//! Run with: cargo run --release -p ponko_bench --bin replication [-- runs [scenario]]
//! (`scenario`: part of a scenario's name, such as `"1 in 20"`).
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use ponko::net::{InterestConfig, InterestTier, NetMessages, Replicate, Replicated, Replication};
use ponko::prelude::*;

const ENTITIES: usize = 1000;
const CLIENTS: usize = 16;
const HZ: u32 = 60;
/// Ticks before measuring: everyone joins and gets the spawn burst.
const WARMUP: u64 = 120;
/// Ticks measured: ten seconds of game time.
const MEASURED: u64 = 600;
const TICKS: u64 = WARMUP + MEASURED;

/// What every entity replicates: a position on a 2 km square to the
/// centimeter, a heading, and a health that never changes.
#[derive(Component, Replicate, Clone, Copy, Default)]
struct Mover {
    #[replicate(min = -1000.0, max = 1000.0, step = 0.01)]
    at: Vec2,
    #[replicate(angle)]
    heading: f32,
}

#[derive(Component, Replicate, Clone, Copy, Default)]
struct Health(#[replicate(bits = 8)] u8);

/// Which entity this is, for who moves when.
#[derive(Component, Clone, Copy)]
struct Index(u32);

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    /// Each entity moves one tick in this many (1: all, every tick).
    move_every: u32,
    budget: u32,
    interest: bool,
}

const SCENARIOS: [Scenario; 5] = [
    Scenario {
        name: "all move, no budget",
        move_every: 1,
        budget: u32::MAX,
        interest: false,
    },
    Scenario {
        name: "all move, 64 KB/s budget",
        move_every: 1,
        budget: ponko::net::DEFAULT_BUDGET,
        interest: false,
    },
    Scenario {
        name: "1 in 10 move, 64 KB/s",
        move_every: 10,
        budget: ponko::net::DEFAULT_BUDGET,
        interest: false,
    },
    Scenario {
        name: "1 in 20 move, 64 KB/s",
        move_every: 20,
        budget: ponko::net::DEFAULT_BUDGET,
        interest: false,
    },
    Scenario {
        name: "all move, interest rings",
        move_every: 1,
        budget: ponko::net::DEFAULT_BUDGET,
        interest: true,
    },
];

fn declarations(s: Scenario) -> Replication {
    let r = Replication::new()
        .component::<Mover>()
        .component::<Health>()
        .budget(s.budget);
    if !s.interest {
        return r;
    }
    // Each client sees about a fifth of the square: near entities every
    // tick, farther ones every 4th.
    r.interest(InterestConfig {
        tiers: vec![
            InterestTier {
                radius: 250.0,
                every: 1,
            },
            InterestTier {
                radius: 500.0,
                every: 4,
            },
        ],
        ..InterestConfig::default()
    })
    .position(|world, e| world.get::<Mover>(e).map(|m| m.at.extend(0.0)))
}

/// Takes turns: one side plays a whole frame, then the next.
struct Turns {
    turn: Mutex<usize>,
    changed: Condvar,
    sides: usize,
}

impl Turns {
    fn wait(&self, me: usize) {
        let mut turn = self.turn.lock().unwrap();
        while *turn != me {
            let (next, waited) = self.changed.wait_timeout(turn, Duration::from_secs(60)).unwrap();
            turn = next;
            assert!(!waited.timed_out() || *turn == me, "another side stopped");
        }
    }

    fn pass(&self, me: usize) {
        *self.turn.lock().unwrap() = (me + 1) % self.sides;
        self.changed.notify_all();
    }
}

/// Runs `game` as side `me`; with `timed`, the CPU time of each of its
/// frames goes into it.
fn take_turns(game: Game, turns: Arc<Turns>, me: usize, timed: Option<Arc<Mutex<Vec<Duration>>>>) -> World {
    let after = turns.clone();
    let mut started = Instant::now();
    game.headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            if let Some(times) = &timed
                && ctx.time.sim_tick() > WARMUP
            {
                times.lock().unwrap().push(started.elapsed());
            }
            after.pass(me);
            if ctx.time.sim_tick() < TICKS {
                after.wait(me);
            }
            started = Instant::now();
        })
        .run_headless(TICKS, move |_| turns.wait(me))
}

/// Replication bytes sent to each client, at the end of the warm-up and
/// at the end.
#[derive(Default)]
struct Sent {
    start: Vec<u64>,
    end: Vec<u64>,
}

fn server(s: Scenario, net: NetMessages) -> Game {
    let mut movers: Vec<Entity> = Vec::new();
    Game::new("server", 1, 1)
        .fixed_hz(HZ)
        .messages(net)
        .replication(declarations(s))
        .sim_system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            if tick == 0 {
                ctx.world.insert_resource(Sent::default());
                let side = (ENTITIES as f32).sqrt().ceil() as usize;
                for i in 0..ENTITIES {
                    let cell = vec2((i % side) as f32, (i / side) as f32) / side as f32;
                    let at = (cell * 2.0 - Vec2::ONE) * 900.0;
                    ctx.world.spawn((
                        Mover { at, heading: i as f32 },
                        Health(100),
                        Index(i as u32),
                        Replicated::new(),
                    ));
                }
            }
            // Walk in wide circles; with `move_every`, a few at a time. Only
            // the movers are borrowed mutably, so only they count as changed.
            let every = u64::from(s.move_every);
            movers.clear();
            ctx.world.query::<(Entity, &Index)>().for_each(|(e, i)| {
                if (tick + u64::from(i.0)).is_multiple_of(every) {
                    movers.push(e);
                }
            });
            for &e in &movers {
                if let Some(m) = ctx.world.get_mut::<Mover>(e) {
                    m.heading = (m.heading + 0.02 * every as f32).rem_euclid(std::f32::consts::TAU);
                    m.at += Vec2::from_angle(m.heading) * 1.5 * every as f32;
                }
            }
            let clients = ctx.net.replication.clients();
            if s.interest {
                for (k, &peer) in clients.iter().enumerate() {
                    let a = k as f32 / CLIENTS as f32 * std::f32::consts::TAU;
                    let at = Vec2::from_angle(a) * 500.0;
                    ctx.net.replication.set_viewer(peer, Some(at.extend(0.0)));
                }
            }
            if tick == WARMUP || tick == TICKS - 1 {
                let bytes: Vec<u64> = clients
                    .iter()
                    .map(|&p| ctx.net.replication.peer_stats(p).map_or(0, |s| s.bytes_sent))
                    .collect();
                let sent = ctx.world.resource_mut::<Sent>();
                if tick == WARMUP {
                    sent.start = bytes
                } else {
                    sent.end = bytes
                }
            }
        })
}

/// What a client received on the wire, at the end of the warm-up and at
/// the end, and how many entities it holds.
#[derive(Default, Clone, Copy)]
struct Received {
    start: u64,
    end: u64,
    replicas: usize,
}

fn client(s: Scenario, net: NetMessages) -> Game {
    Game::new("client", 1, 1)
        .fixed_hz(HZ)
        .messages(net)
        .replication(declarations(s))
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let bytes = ctx.net.messages.as_ref().map_or(0, |m| m.stats().bytes_received);
            let mut got = ctx.world.get_resource::<Received>().copied().unwrap_or_default();
            if tick == WARMUP {
                got.start = bytes;
            }
            if tick == TICKS - 1 {
                got.end = bytes;
                let mut n = 0;
                ctx.world.query::<&ponko::net::Replica>().for_each(|_| n += 1);
                got.replicas = n;
            }
            ctx.world.insert_resource(got);
        })
}

struct Run {
    payload: Vec<u64>,
    wire: Vec<u64>,
    replicas: Vec<usize>,
    frames: Vec<Duration>,
}

fn run(s: Scenario) -> Run {
    let mut nets: Vec<NetMessages> = NetMessages::loopback(CLIENTS + 1)
        .into_iter()
        .map(NetMessages::game_clock)
        .collect();
    let host = nets.remove(0);
    let turns = Arc::new(Turns {
        turn: Mutex::new(0),
        changed: Condvar::new(),
        sides: CLIENTS + 1,
    });
    let clients: Vec<_> = nets
        .into_iter()
        .enumerate()
        .map(|(n, net)| {
            let turns = turns.clone();
            std::thread::spawn(move || {
                let world = take_turns(client(s, net), turns, n + 1, None);
                *world.resource::<Received>()
            })
        })
        .collect();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let world = take_turns(server(s, host), turns, 0, Some(frames.clone()));
    let got: Vec<Received> = clients.into_iter().map(|c| c.join().unwrap()).collect();
    let sent = world.resource::<Sent>();
    assert_eq!(sent.start.len(), CLIENTS, "every client joined during the warm-up");
    let frames = std::mem::take(&mut *frames.lock().unwrap());
    Run {
        payload: sent.end.iter().zip(&sent.start).map(|(e, s)| e - s).collect(),
        wire: got.iter().map(|g| g.end - g.start).collect(),
        replicas: got.iter().map(|g| g.replicas).collect(),
        frames,
    }
}

fn per_second(bytes: u64) -> f64 {
    bytes as f64 / (MEASURED as f64 / f64::from(HZ))
}

fn spread(values: &[u64]) -> String {
    let (lo, hi) = (values.iter().min().unwrap(), values.iter().max().unwrap());
    let mean = values.iter().sum::<u64>() as f64 / values.len() as f64;
    if lo == hi {
        format!("{:.0}", per_second(*lo))
    } else {
        format!(
            "{:.0} ({:.0}..{:.0})",
            per_second(mean as u64),
            per_second(*lo),
            per_second(*hi)
        )
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let runs: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(3);
    // An optional second argument runs only the scenarios whose name has it.
    let only = std::env::args().nth(2).unwrap_or_default();
    println!(
        "replication: {ENTITIES} entities, {CLIENTS} clients, {HZ} Hz, {MEASURED} ticks measured after {WARMUP}, {runs} runs"
    );
    println!(
        "{:<26} {:>22} {:>22} {:>9} {:>34}",
        "scenario", "payload B/s/client", "wire B/s/client", "replicas", "server ms/tick: median p90 (runs)"
    );
    for s in SCENARIOS.into_iter().filter(|s| s.name.contains(only.as_str())) {
        let mut medians = Vec::new();
        let mut p90s = Vec::new();
        let mut first: Option<Run> = None;
        for _ in 0..runs {
            let mut r = run(s);
            r.frames.sort();
            medians.push(percentile(&r.frames, 0.5).as_secs_f64() * 1e3);
            p90s.push(percentile(&r.frames, 0.9).as_secs_f64() * 1e3);
            if let Some(f) = &first {
                // The traffic is exact: the same on every run.
                assert_eq!(
                    (&f.payload, &f.wire),
                    (&r.payload, &r.wire),
                    "the bytes differ between runs"
                );
            } else {
                first = Some(r);
            }
        }
        let r = first.unwrap();
        medians.sort_by(f64::total_cmp);
        p90s.sort_by(f64::total_cmp);
        let replicas = r.replicas.iter().sum::<usize>() / r.replicas.len();
        let cpu = format!(
            "{:.3} {:.3} ({:.3}..{:.3})",
            medians[medians.len() / 2],
            p90s[p90s.len() / 2],
            medians[0],
            medians[medians.len() - 1]
        );
        println!(
            "{:<26} {:>22} {:>22} {:>9} {:>34}",
            s.name,
            spread(&r.payload),
            spread(&r.wire),
            replicas,
            cpu
        );
    }
}
