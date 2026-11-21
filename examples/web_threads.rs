//! Threads on the web: the browser fixture and benchmark for web builds
//! with threads (`web/smoke/threads.mjs`; the guide's "Threads on the
//! web"). Build it with `web/build.sh --threads web_threads`.
//!
//! **Check** (the default): a parallel query over 20,000 entities every
//! tick, four view systems the parallel scheduler batches together (its
//! threshold at zero, so a batch goes to the workers whenever there are
//! any), jobs (one waited for during setup and one every 30 ticks), and an
//! asset group of a texture and a file decoded with `AssetGroup::with`,
//! both from `assets/`. It publishes which threads ran each, for the runner
//! (see `common/web_smoke.rs`):
//!
//! `{"fixture":"web_threads","ticks":160,"threads":7,"isolated":true,"par_threads":7,"par_on_page":false,"batch_threads":4,"batch_threaded_runs":160,"wait_off_page":true,"job_off_page":true,"group":"ready","group_off_page":true,"group_bytes":966,"checksum":...}`
//!
//! `threads` is what the engine started (`ponko::web::threads()`, 0 when it
//! runs on the page's thread alone); `par_threads` and `batch_threads`
//! count the threads other than the page's that ran query items and batch
//! systems; `checksum` sums every position at tick 120, the same however
//! many threads ran them. `?smoke_case=worker-panic` panics inside the
//! parallel query at tick 30: the game must stop with an error, not hang.
//!
//! `pads` lists the controllers the engine sees (`threads.mjs --case
//! gamepads` feeds the page fake browser gamepads): `[{"slot":0,"name":"...",
//! "family":"Xbox","vendor":69,"product":2835,"down":["South"],"axes":{"lx":0.5,
//! "ly":-1,"rx":0,"ry":0,"lt":0.25,"rt":0}}]`, and `pad_presses` counts the
//! South presses seen.
//!
//! **Bench** (`?bench=<workload>&threads=<n>`, `threads=1` for one
//! thread): times one workload of the native simulation benchmarks
//! (`crates/ponko_bench`), then publishes `bench` (the workload, the
//! threads, and every timed run in milliseconds) and `done`:
//!
//! - `par`: `ecs/par-100k`'s steering, `par_for_each` over 100,000 entities;
//! - `sweep`: `schedule/sweep/8x16000/heavy`, eight independent view
//!   systems in one batch;
//! - `interleaved`: `schedule/interleaved/4x16000/heavy`, producers and
//!   consumers in two batches;
//! - `jobs`: 16 jobs of about the same work, from spawning the first to
//!   having all results.
//!
//! On the desktop it opens a window like any other example (`cargo run
//! --example web_threads`) and prints its state once a second.

use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Mutex;
use std::thread::ThreadId;
use std::time::Duration;

use ponko::access::SystemAccess;
use ponko::assets::{AssetGroup, GroupId, GroupState};
use ponko::input::Input;
use ponko::prelude::*;
use ponko::streaming::Job;
use ponko::system_view::SystemView;
use serde_json::json;
use web_time::Instant;

#[path = "common/web_smoke.rs"]
mod smoke;

const DT: f32 = 1.0 / 60.0;

#[derive(Clone, Copy, Component)]
struct Pos {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Component)]
struct Vel {
    x: f32,
    y: f32,
}

/// Two floats per entity; every `N` is its own component, so view systems
/// over different `N` don't conflict (as in `crates/ponko_bench`'s
/// `schedule.rs`).
#[derive(Clone, Copy, Component)]
struct Val<const N: usize>(f32, f32);

/// The threads that ran query items, batch systems: each thread adds
/// itself once.
static PAR_THREADS: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());
static BATCH_THREADS: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

thread_local! {
    static SEEN_PAR: Cell<bool> = const { Cell::new(false) };
    static SEEN_BATCH: Cell<bool> = const { Cell::new(false) };
}

/// Adds this thread to `list` the first time it gets here.
fn note(seen: &'static std::thread::LocalKey<Cell<bool>>, list: &Mutex<Vec<ThreadId>>) {
    if !seen.get() {
        seen.set(true);
        list.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(std::thread::current().id());
    }
}

/// `(threads other than the page's, whether the page's own is among them)`.
fn count(list: &Mutex<Vec<ThreadId>>, page: ThreadId) -> (usize, bool) {
    let list = list.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    (list.iter().filter(|&&t| t != page).count(), list.contains(&page))
}

/// `ecs/par-*`'s steering: a few transcendental calls per entity.
fn steer(p: &mut Pos, v: &mut Vel) {
    let a = (p.x * 0.013 + p.y * 0.007).sin() * std::f32::consts::PI;
    let (s, c) = a.sin_cos();
    v.x = v.x * 0.9 + c * 0.1;
    v.y = v.y * 0.9 + s * 0.1;
    p.x = (p.x + v.x * DT).rem_euclid(4096.0);
    p.y = (p.y + v.y * DT).rem_euclid(4096.0);
}

/// `schedule.rs`'s heavy system: roughly 20 ns per entity natively.
fn heavy<const N: usize>(sys: &mut SystemView) {
    note(&SEEN_BATCH, &BATCH_THREADS);
    let dt = black_box(DT);
    sys.world.query::<&mut Val<N>>().for_each(|v| {
        for _ in 0..4 {
            let (s, c) = (v.0 * 0.001 + v.1).sin_cos();
            v.0 += (c - v.1 * s) * dt;
            v.1 = (v.1 + (c + v.0 * s) * dt).rem_euclid(4096.0);
        }
    });
}

/// Reads `Val<N>` and writes `Val<M>`.
fn link<const N: usize, const M: usize>(sys: &mut SystemView) {
    let dt = black_box(DT);
    sys.world
        .query::<(&Val<N>, &mut Val<M>)>()
        .for_each(|(a, b)| b.0 += a.0 * dt);
}

/// A job's work: about as much as 1,000 entities of `heavy`.
fn job_work(seed: u32) -> f32 {
    let mut v = (seed as f32, 1.0f32);
    for _ in 0..4_000 {
        let (s, c) = (v.0 * 0.001 + v.1).sin_cos();
        v.0 += (c - v.1 * s) * DT;
        v.1 = (v.1 + (c + v.0 * s) * DT).rem_euclid(4096.0);
    }
    v.0
}

/// A page query parameter, or `None` (also on the desktop).
fn param(name: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        search
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_string))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = name;
        None
    }
}

/// The worker threads the engine runs on (0 on the page's thread alone);
/// on the desktop, rayon's.
fn engine_threads() -> usize {
    #[cfg(target_arch = "wasm32")]
    return ponko::web::threads();
    #[cfg(not(target_arch = "wasm32"))]
    return std::thread::available_parallelism().map_or(1, |n| n.get());
}

/// Whether the page is cross-origin isolated (always `true` on the desktop).
fn isolated() -> bool {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Reflect::get(&js_sys::global(), &"crossOriginIsolated".into()).is_ok_and(|v| v.is_truthy());
    #[cfg(not(target_arch = "wasm32"))]
    return true;
}

fn main() {
    let mut game = Game::new("Ponko: web threads", 640, 360).fixed_hz(60);
    if let Some(threads) = param("threads").and_then(|t| t.parse().ok()) {
        game = game.web_threads(threads);
    }
    match param("bench") {
        Some(workload) => bench(game, workload),
        None => check(game),
    }
}

/// The controllers the engine sees, for the runner's `gamepads` case.
fn pads(input: &Input) -> serde_json::Value {
    use GamepadButton as B;
    const BUTTONS: [GamepadButton; 16] = [
        B::South,
        B::East,
        B::North,
        B::West,
        B::LeftBumper,
        B::RightBumper,
        B::LeftTrigger,
        B::RightTrigger,
        B::Select,
        B::Start,
        B::LeftStick,
        B::RightStick,
        B::DPadUp,
        B::DPadDown,
        B::DPadLeft,
        B::DPadRight,
    ];
    let pads: Vec<_> = input
        .gamepads()
        .iter()
        .map(|&pad| {
            let info = input.gamepad_info(pad);
            let down: Vec<_> = BUTTONS
                .iter()
                .filter(|&&b| input.gamepad_down(pad, b))
                .map(|b| format!("{b:?}"))
                .collect();
            let axis = |a| input.gamepad_axis(pad, a);
            json!({
                "slot": pad.slot(),
                "name": info.map(|i| i.name.clone()),
                "family": info.map(|i| format!("{:?}", i.family)),
                "vendor": info.and_then(|i| i.vendor_id),
                "product": info.and_then(|i| i.product_id),
                "down": down,
                "axes": {
                    "lx": axis(GamepadAxis::LeftStickX),
                    "ly": axis(GamepadAxis::LeftStickY),
                    "rx": axis(GamepadAxis::RightStickX),
                    "ry": axis(GamepadAxis::RightStickY),
                    "lt": axis(GamepadAxis::LeftTrigger),
                    "rt": axis(GamepadAxis::RightTrigger),
                },
            })
        })
        .collect();
    json!(pads)
}

/// What the check publishes, filled in by its systems.
#[derive(Default)]
struct Check {
    ticks: u64,
    pad_presses: u64,
    checksum: Option<f64>,
    wait_off_page: Option<bool>,
    job_off_page: Option<bool>,
    job: Option<Job<ThreadId>>,
    schedule_threads: Option<bool>,
    batch_threaded_runs: u64,
    group: Option<GroupId>,
    /// `ready`, or `failed: <why>`, once the group is done.
    group_state: Option<String>,
    group_off_page: Option<bool>,
    group_bytes: Option<usize>,
}

/// The asset group's file decoded with `with`, and what its decoder returns.
const GROUP_FILE: &str = "audio_mixer.json";
type Decoded = (ThreadId, usize);

fn check(game: Game) {
    let state = Rc::new(std::cell::RefCell::new(Check::default()));
    let page = std::thread::current().id();
    let panic_at = (param("smoke_case").as_deref() == Some("worker-panic")).then_some(30);
    let mut game = game.parallel_systems(true).parallel_threshold(Duration::ZERO);
    // Four view systems with nothing in common: one batch.
    macro_rules! batch {
        ($($n:literal)*) => {$(
            game = game.add(Phase::AfterPhysics, heavy::<$n>.access(SystemAccess::new().write::<Val<$n>>()));
        )*};
    }
    batch!(0 1 2 3);
    let sim = state.clone();
    let frame = state.clone();
    let mut smoke = None;
    game.sim_system(move |ctx| {
        let mut s = sim.borrow_mut();
        s.ticks += 1;
        let tick = s.ticks;
        ctx.world.query::<(&mut Pos, &mut Vel)>().par_for_each(|(p, v)| {
            note(&SEEN_PAR, &PAR_THREADS);
            if panic_at == Some(tick) {
                panic!("web_threads: panicking inside the parallel query, as ?smoke_case=worker-panic asked");
            }
            steer(p, v);
        });
        if tick == 120 {
            let mut sum = 0.0f64;
            ctx.world
                .query::<&Pos>()
                .for_each(|p| sum += f64::from(p.x) + f64::from(p.y));
            s.checksum = Some(sum);
        }
        if tick.is_multiple_of(30) && s.job.is_none() {
            s.job = Some(Job::spawn(|| std::thread::current().id()));
        }
    })
    .system(move |ctx| {
        let mut s = frame.borrow_mut();
        if let Some(job) = &mut s.job
            && let Some(thread) = job.take()
        {
            s.job_off_page = Some(thread != page);
            s.job = None;
        }
        let (par_threads, par_on_page) = count(&PAR_THREADS, page);
        let (batch_threads, batch_on_page) = count(&BATCH_THREADS, page);
        if s.ticks.is_multiple_of(30) || s.schedule_threads.is_none() {
            let d = ctx.ecs_diagnostics();
            s.schedule_threads = Some(d.schedule.threads);
            s.batch_threaded_runs = d.schedule.stages.iter().map(|st| st.threaded_runs).sum();
        }
        if let Some(id) = s.group
            && s.group_state.is_none()
        {
            match ctx.assets.group_state(id) {
                GroupState::Loading => {}
                GroupState::Ready => {
                    if let Some((thread, bytes)) = ctx.assets.take_group_value::<Decoded>(id, GROUP_FILE) {
                        s.group_off_page = Some(thread != page);
                        s.group_bytes = Some(bytes);
                    }
                    s.group_state = Some("ready".into());
                }
                other => {
                    let why = ctx.assets.group_error(id).map(ToString::to_string).unwrap_or_default();
                    s.group_state = Some(format!("{}: {why}", other.name()));
                }
            }
        }
        let batch_threaded_runs = s.batch_threaded_runs;
        s.pad_presses += ctx
            .input
            .gamepads()
            .iter()
            .filter(|&&p| ctx.input.gamepad_pressed(p, GamepadButton::South))
            .count() as u64;
        let extra = json!({
            "threads": engine_threads(),
            "isolated": isolated(),
            "par_threads": par_threads,
            "par_on_page": par_on_page,
            "batch_threads": batch_threads,
            "batch_on_page": batch_on_page,
            "batch_threaded_runs": batch_threaded_runs,
            "schedule_threads": s.schedule_threads,
            "wait_off_page": s.wait_off_page,
            "job_off_page": s.job_off_page,
            "group": s.group_state,
            "group_off_page": s.group_off_page,
            "group_bytes": s.group_bytes,
            "checksum": s.checksum,
            "pads": pads(&ctx.input),
            "pad_presses": s.pad_presses,
        });
        let ticks = s.ticks;
        drop(s);
        if let Some(smoke) = &mut smoke {
            smoke::Smoke::frame(smoke, ctx, ticks, extra);
        } else {
            smoke = Some(smoke::Smoke::new("web_threads"));
        }
        ctx.gfx.clear(Color::rgb(0.08, 0.09, 0.11));
        ctx.gfx.draw_rect(
            Rect::new(20.0, 20.0, 120.0 + (ticks % 200) as f32, 24.0),
            Color::rgb(0.4, 0.8, 0.5),
        );
    })
    .setup(move |ctx| {
        spawn_agents(ctx, 20_000);
        spawn_vals::<4>(ctx, 16_000);
        // Waited for on the page's thread, which spins (it may not block).
        let thread = Job::spawn(|| std::thread::current().id()).wait();
        // Files from assets/ (assets/index.json on the web), read and
        // decoded by jobs.
        let group = AssetGroup::new("web_threads")
            .texture("crab.png")
            .with(GROUP_FILE, |bytes| {
                Ok::<Decoded, _>((std::thread::current().id(), bytes.len()))
            });
        let mut s = state.borrow_mut();
        s.wait_off_page = Some(thread != page);
        s.group = Some(ctx.assets.start_group(group));
    })
    .run();
}

/// `n` steering agents, spread over the field.
fn spawn_agents(ctx: &mut Ctx, n: usize) {
    for i in 0..n {
        let f = i as f32;
        ctx.world.spawn((
            Pos {
                x: (f * 7.31) % 4096.0,
                y: (f * 3.17) % 4096.0,
            },
            Vel { x: 1.0, y: 0.5 },
        ));
    }
}

/// `entities` entities with the first `COMPONENTS` `Val`s each.
fn spawn_vals<const COMPONENTS: usize>(ctx: &mut Ctx, entities: usize) {
    for i in 0..entities {
        let v = (i as f32, 1.0);
        let e = ctx.world.spawn((Val::<0>(v.0, v.1),));
        if COMPONENTS > 1 {
            ctx.world.insert(e, Val::<1>(v.0, v.1));
        }
        if COMPONENTS > 2 {
            ctx.world.insert(e, Val::<2>(v.0, v.1));
        }
        if COMPONENTS > 3 {
            ctx.world.insert(e, Val::<3>(v.0, v.1));
        }
        if COMPONENTS > 4 {
            ctx.world.insert(e, Val::<4>(v.0, v.1));
            ctx.world.insert(e, Val::<5>(v.0, v.1));
            ctx.world.insert(e, Val::<6>(v.0, v.1));
            ctx.world.insert(e, Val::<7>(v.0, v.1));
        }
    }
}

/// Untimed runs before the timed ones, and timed runs.
const WARMUP: usize = 20;
const RUNS: usize = 60;

/// The bench's timings, from its systems.
#[derive(Default)]
struct Timings {
    started: Option<Instant>,
    runs: Vec<f64>,
    seen: usize,
}

impl Timings {
    /// Records one run of `ms` (after the warmup).
    fn record(&mut self, ms: f64) {
        self.seen += 1;
        if self.seen > WARMUP && self.runs.len() < RUNS {
            self.runs.push(ms);
        }
    }
}

fn bench(game: Game, workload: String) {
    let timings = Rc::new(std::cell::RefCell::new(Timings::default()));
    let mut game = game.parallel_systems(true);
    let entities = match workload.as_str() {
        "par" => 100_000,
        "jobs" => 0,
        _ => 16_000,
    };
    // A tick's time, from its first phase to its last (as `schedule.rs`).
    let start = timings.clone();
    let end = timings.clone();
    let tick_timed = matches!(workload.as_str(), "sweep" | "interleaved");
    if tick_timed {
        game = game
            .add(Phase::Input, move |_ctx: &mut Ctx| {
                start.borrow_mut().started = Some(Instant::now())
            })
            .add(Phase::SimLast, move |_ctx: &mut Ctx| {
                let mut t = end.borrow_mut();
                if let Some(started) = t.started.take() {
                    t.record(started.elapsed().as_secs_f64() * 1e3);
                }
            });
    }
    match workload.as_str() {
        "sweep" => {
            macro_rules! sweep {
                ($($n:literal)*) => {$(
                    game = game.add(Phase::AfterPhysics, heavy::<$n>.access(SystemAccess::new().write::<Val<$n>>()));
                )*};
            }
            sweep!(0 1 2 3 4 5 6 7);
        }
        "interleaved" => {
            macro_rules! pairs {
                ($($p:literal $c:literal)*) => {$(
                    game = game
                        .add(Phase::AfterPhysics, heavy::<$p>.access(SystemAccess::new().write::<Val<$p>>()))
                        .add(
                            Phase::AfterPhysics,
                            link::<$p, $c>.access(SystemAccess::new().read::<Val<$p>>().write::<Val<$c>>()),
                        );
                )*};
            }
            pairs!(0 4  1 5  2 6  3 7);
        }
        "par" => {
            let t = timings.clone();
            game = game.sim_system(move |ctx| {
                let started = Instant::now();
                ctx.world
                    .query::<(&mut Pos, &mut Vel)>()
                    .par_for_each(|(p, v)| steer(p, v));
                t.borrow_mut().record(started.elapsed().as_secs_f64() * 1e3);
            });
        }
        "jobs" => {
            let t = timings.clone();
            game = game.system(move |_ctx| {
                let started = Instant::now();
                let jobs: Vec<Job<f32>> = (0..16).map(|i| Job::spawn(move || job_work(i))).collect();
                let sum: f32 = jobs.into_iter().map(Job::wait).sum();
                black_box(sum);
                t.borrow_mut().record(started.elapsed().as_secs_f64() * 1e3);
            });
        }
        other => panic!("web_threads: unknown workload `{other}` (par, sweep, interleaved or jobs)"),
    }
    let frame = timings.clone();
    let mut smoke = None;
    let mut frames = 0u64;
    game.system(move |ctx| {
        frames += 1;
        let t = frame.borrow();
        let done = t.runs.len() >= RUNS;
        let extra = json!({
            "threads": engine_threads(),
            "isolated": isolated(),
            "done": done,
            "bench": {"workload": workload, "threads": engine_threads(), "runs_ms": if done { t.runs.clone() } else { Vec::new() }},
        });
        drop(t);
        if let Some(smoke) = &mut smoke {
            smoke::Smoke::frame(smoke, ctx, frames, extra);
        } else {
            smoke = Some(smoke::Smoke::new("web_threads"));
        }
        ctx.gfx.clear(Color::rgb(0.08, 0.09, 0.11));
    })
    .setup(move |ctx| match entities {
        0 => {}
        100_000 => spawn_agents(ctx, entities),
        _ => spawn_vals::<8>(ctx, entities),
    })
    .run();
}
