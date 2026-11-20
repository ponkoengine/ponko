//! Legions: two armies of 50,000 fight it out on a map of rocks, lakes and
//! mud, and every battle can be recorded and replayed bit for bit.
//! Soldiers charge and strike, archers stop at range and shoot arrows that
//! fly, land and hit whoever stands there; the fallen stay on the field for
//! a while. You command red; blue attacks.
//!
//!   `cargo run --release --example legions`
//!
//! Mouse: left click sends red to that point; drag with the right button to
//! pan, scroll to zoom. Keys: A attack, H hold, R new round, Space pause,
//! 1-3 speed, Escape quit.
//!
//! The rules live in `game.rs`, which `tests/legions.rs` also runs without a
//! window. This file reads the command line:
//!
//!   `legions [--units N] [--seed S]`                  play (100,000 units)
//!   `legions --record replay.json`                    play and record the orders
//!   `legions --replay replay.json`                    play it again headless and check its hash
//!   `legions --replay replay.json --watch`            watch it in a window
//!   `legions --headless --ticks T [--units N]`        time T ticks, print the world hash
//!   `--threads N` runs the simulation on N threads (it gives the same hash).
//!
//! Package it with `ponko package --example legions` (the root `Cargo.toml`
//! says what goes in); the web build is `web/build.sh legions`.

mod game;

#[cfg(target_arch = "wasm32")]
fn main() {
    // One core in a browser: a smaller battle.
    game::build(10_000, game::SEED).run(|_| {});
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match Options::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("legions: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Some(threads) = options.threads
        && let Err(e) = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global()
    {
        eprintln!("legions: --threads: {e}");
        std::process::exit(2);
    }
    if let Some(path) = &options.replay {
        let replay = match game::Replay::load(path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("legions: {e}");
                std::process::exit(2);
            }
        };
        if options.watch {
            game::with_replay(game::build(replay.units, replay.seed), &replay).run(|_| {});
            return;
        }
        std::process::exit(check_replay(&replay));
    }
    if options.headless {
        headless(options.units, options.seed, options.ticks);
        return;
    }
    let mut play = game::build(options.units, options.seed);
    if let Some(path) = options.record {
        println!("legions: recording to {}", path.display());
        play = game::with_recorder(play, path, options.units, options.seed, 600);
    }
    play.run(|_| {});
}

#[cfg(not(target_arch = "wasm32"))]
const USAGE: &str = "usage: legions [--units N] [--seed S] [--threads N] [--record FILE | --replay FILE [--watch] | --headless [--ticks T]]";

#[cfg(not(target_arch = "wasm32"))]
struct Options {
    units: usize,
    seed: u64,
    ticks: u64,
    threads: Option<usize>,
    headless: bool,
    record: Option<std::path::PathBuf>,
    replay: Option<std::path::PathBuf>,
    watch: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options {
            units: game::DEFAULT_UNITS,
            seed: game::SEED,
            ticks: 600,
            threads: None,
            headless: false,
            record: None,
            replay: None,
            watch: false,
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
            let number = |v: &String| v.parse::<u64>().map_err(|_| format!("{arg}: not a number: {v}"));
            match arg.as_str() {
                "--units" => o.units = (number(value()?)? as usize).max(2),
                "--seed" => o.seed = number(value()?)?,
                "--ticks" => o.ticks = number(value()?)?,
                "--threads" => o.threads = Some((number(value()?)? as usize).max(1)),
                "--headless" => o.headless = true,
                "--record" => o.record = Some(value()?.into()),
                "--replay" => o.replay = Some(value()?.into()),
                "--watch" => o.watch = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(o)
    }
}

/// Plays a replay headless; 0 if it ends with the hash it recorded.
#[cfg(not(target_arch = "wasm32"))]
fn check_replay(replay: &game::Replay) -> i32 {
    let start = std::time::Instant::now();
    let (world, hash) = game::play_replay(replay);
    let score = world.resource::<game::Score>();
    println!(
        "legions: replayed {} ticks of {} units ({} orders) on {} threads in {:.2} s: red {} blue {}, rounds {}-{}",
        replay.ticks,
        replay.units,
        replay.orders.len(),
        rayon::current_num_threads(),
        start.elapsed().as_secs_f64(),
        score.alive[0],
        score.alive[1],
        score.rounds_won[0],
        score.rounds_won[1],
    );
    if replay.expected_hash() == Some(hash) {
        println!("legions: world hash {} matches", game::hash_hex(hash));
        0
    } else {
        eprintln!(
            "legions: world hash {} does not match the recorded {}",
            game::hash_hex(hash),
            replay.hash
        );
        1
    }
}

/// Times `ticks` ticks of the rules alone (no window, no sprites), then
/// prints the tick times and the world hash.
#[cfg(not(target_arch = "wasm32"))]
fn headless(units: usize, seed: u64, ticks: u64) {
    use ponko::prelude::*;
    use std::time::Instant;
    /// When the current tick started, every tick's time in ms, and each
    /// system's total time in ms (from `ctx.perf`).
    struct Clock(Option<Instant>, Vec<f32>, Vec<(String, f32)>);
    let setup_start = Instant::now();
    let mut setup_secs = 0.0;
    let world = game::headless(units, seed)
        .add(Phase::Input, |ctx: &mut Ctx| {
            ctx.world.resource_mut::<Clock>().0 = Some(Instant::now());
        })
        .add(Phase::SimLast, |ctx: &mut Ctx| {
            let timings: Vec<(&'static str, f32)> = ctx
                .perf
                .system_timings()
                .iter()
                .filter(|t| t.sim)
                .map(|t| (t.name, t.last_ms))
                .collect();
            let clock = ctx.world.resource_mut::<Clock>();
            if let Some(start) = clock.0.take() {
                clock.1.push(start.elapsed().as_secs_f32() * 1000.0);
            }
            for (name, ms) in timings {
                let name = name.rsplit("::").next().unwrap_or(name);
                match clock.2.iter_mut().find(|(n, _)| n == name) {
                    Some((_, total)) => *total += ms,
                    None => clock.2.push((name.to_string(), ms)),
                }
            }
        })
        .run_headless(ticks, |ctx| {
            ctx.world
                .insert_resource(Clock(None, Vec::with_capacity(ticks as usize), Vec::new()));
            setup_secs = setup_start.elapsed().as_secs_f64();
        });
    let mut times = world.resource::<Clock>().1.clone();
    times.sort_by(f32::total_cmp);
    let at = |p: f32| {
        times
            .get(((times.len() as f32 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let mean = times.iter().sum::<f32>() / times.len().max(1) as f32;
    let b = world.resource::<game::Battle>();
    let s = world.resource::<game::Score>();
    println!(
        "legions: {units} units, {}x{} cells, {ticks} ticks on {} threads (setup {:.2} s)",
        b.nav.size().x,
        b.nav.size().y,
        rayon::current_num_threads(),
        setup_secs
    );
    println!(
        "  tick ms: mean {mean:.2}, p50 {:.2}, p95 {:.2}, max {:.2}",
        at(0.5),
        at(0.95),
        at(1.0)
    );
    let mut systems = world.resource::<Clock>().2.clone();
    systems.sort_by(|a, b| b.1.total_cmp(&a.1));
    let systems: Vec<String> = systems
        .iter()
        .filter(|(_, ms)| *ms / ticks.max(1) as f32 >= 0.005)
        .map(|(name, ms)| format!("{name} {:.2}", ms / ticks.max(1) as f32))
        .collect();
    println!("  per system, ms per tick: {}", systems.join(", "));
    println!(
        "  red {} blue {}, fallen {} / {}, arrows in flight {}, round {}",
        s.alive[0], s.alive[1], s.fallen[0], s.fallen[1], s.arrows, s.round
    );
    println!("  world hash {}", game::hash_hex(game::world_hash(&world)));
}
