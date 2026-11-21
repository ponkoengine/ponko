//! Threads on the web: the browser fixture and benchmark for web builds
//! with threads (`web/smoke/threads.mjs`; the guide's "Threads on the
//! web"). Build it with `web/build.sh --threads --features
//! 3d,physics3d,bench-internals web_threads` (without those features it
//! leaves out the 3D culling, the light bake, the 2D skeleton drawing and
//! the 3D characters).
//!
//! **Check** (the default): a parallel query over 20,000 entities every
//! tick, four view systems the parallel scheduler batches together (its
//! threshold at zero, so a batch goes to the workers whenever there are
//! any), jobs (one waited for during setup and one every 30 ticks), a
//! moving root with 4,000 attached children whose transforms are tracked
//! (so the hierarchy places them by lookups, on the workers when the level
//! is worth sharing, through the same runner as the parallel query), the
//! engine's own parallel passes (a spatial grid of 200,000 entries rebuilt
//! and a 256 × 256 flow field computed every 10 ticks, the 3D frame of
//! 20,000 models collected and culled every 10 ticks, 256 2D skeletons
//! drawn every 10 ticks, 32 3D characters walking every tick, a small
//! scene's light baked once, all through that runner, and the world saved
//! as JSON every tick while profiling, which stays on the page's thread),
//! and an
//! asset group of a texture and a file decoded with `AssetGroup::with`,
//! both from `assets/`. It publishes which threads ran each, for the runner
//! (see `common/web_smoke.rs`):
//!
//! `{"fixture":"web_threads","ticks":160,"threads":7,"isolated":true,"par_threads":7,"par_on_page":false,"batch_threads":4,"batch_threaded_runs":160,"hier_shared":true,"passes_shared":["flow/shared_directions","spatial/shared_rebuild","three/shared_cull",...],"pass_ms":{"spatial/shared_rebuild":4.1,...},"frame3d":[...],"save_bytes":...,"wait_off_page":true,"job_off_page":true,"group":"ready","group_off_page":true,"group_bytes":966,"checksum":...}`
//!
//! `threads` is what the engine started (`ponko::web::threads()`, 0 when it
//! runs on the page's thread alone); `par_threads` and `batch_threads`
//! count the threads other than the page's that ran query items and batch
//! systems (batch systems not while profiling, when the scheduler runs them
//! on the page's thread); `hier_shared` says whether the hierarchy shared a
//! level out (the engine's `hierarchy/shared_level` scope, timed over 20
//! ticks with profiling on); `passes_shared` lists the engine's passes
//! that went through the frame pool's runner, by the scope each opens there
//! (`spatial/shared_rebuild`, `flow/shared_directions`, `three/shared_cull`,
//! `skeleton2d/shared_draw`, `physics3d/shared_characters`,
//! `three/shared_bake`; and `reflect/shared_save`, which must not open
//! on the web, where the save stays on the page's thread); `pass_ms` is each
//! pass's time in milliseconds (`PASS_TIMES`: the median of the 20 ticks
//! profiled, the bake's one run), timed around the call on the page's
//! thread, so with threads it includes handing the pass to the workers;
//! `frame3d` is what the 3D collection found at
//! tick 120 (instances for the camera and the shadow views, and draw runs;
//! `null` without the features) and `save_bytes` the length of the last JSON
//! save; `checksum` sums every position, every attached child's place, the
//! grid's and flow field's results, the 3D frame's counts, the skeletons'
//! vertices, the characters' places, the bake's hash and the save's
//! length and hash at tick 120, the same however many threads ran them. `?smoke_case=worker-panic` panics inside the
//! parallel query at tick 30: the game must stop with an error, not hang.
//!
//! `pads` lists the controllers the engine sees (`threads.mjs --case
//! gamepads` feeds the page fake browser gamepads): `[{"slot":0,"name":"...",
//! "family":"Xbox","vendor":69,"product":2835,"down":["South"],"axes":{"lx":0.5,
//! "ly":-1,"rx":0,"ry":0,"lt":0.25,"rt":0}}]`, and `pad_presses` counts the
//! South presses seen. Pressing West makes the game rumble that pad for 2.5
//! seconds (`Rumble::new(0.8, 0.3)`), which the runner sees as the calls
//! on the page's fake `vibrationActuator`; `rumble` is the pad's capability.
//! Pressing North rumbles that pad's trigger motors with the weak body
//! motor for 1.5 seconds (`Rumble::new(0.0, 0.2).triggers(0.4, 0.9)`): a
//! pad whose actuator plays `trigger-rumble` gets all four levels, any other
//! the body motor alone; `trigger_rumble` is that capability.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;
use std::time::Duration;

use ponko::access::SystemAccess;
use ponko::assets::{AssetGroup, GroupId, GroupState};
use ponko::attach::Attached;
use ponko::input::{Input, Rumble};
use ponko::prelude::*;
use ponko::streaming::Job;
use ponko::system_view::SystemView;
use serde_json::json;
use web_time::Instant;

#[path = "common/web_smoke.rs"]
mod smoke;

const DT: f32 = 1.0 / 60.0;

/// Children attached to the moving root: enough that the hierarchy's
/// lookup level is shared out on the web.
const CHILDREN: usize = 4_000;

/// The first of the 20 ticks profiled (see `hier_shared`).
const PROFILED_FROM: u64 = 90;

/// The engine's own parallel passes the fixture runs, by the scope each
/// opens on the thread that shares it out (on the web, a worker). The 3D
/// culling runs only in a build with the `3d` and `bench-internals`
/// features (`threads.mjs` builds it so). `reflect/shared_save` is listed
/// so that the runner sees it if it ever opens: in a web build with threads
/// the save stays on the page's thread, where it is faster.
const ENGINE_PASSES: [&str; 7] = [
    "spatial/shared_rebuild",
    "flow/shared_directions",
    "three/shared_cull",
    "reflect/shared_save",
    "skeleton2d/shared_draw",
    "physics3d/shared_characters",
    "three/shared_bake",
];

/// The passes `pass_ms` times, by the scope each opens on the frame pool:
/// the hierarchy's update and the engine's own passes (`reflect/save`: the
/// JSON save, which a web build with threads keeps on the page's thread).
const PASS_TIMES: [&str; 8] = [
    "hierarchy/shared_level",
    "spatial/shared_rebuild",
    "flow/shared_directions",
    "three/shared_cull",
    "reflect/save",
    "skeleton2d/shared_draw",
    "physics3d/shared_characters",
    "three/shared_bake",
];

/// 2D skeletons drawn every `PASS_EVERY` ticks: past the 64 from which
/// their drawing is shared out.
#[cfg(feature = "bench-internals")]
const SKELETONS: usize = 256;

/// 3D characters walking every tick: past the 16 from which their moves
/// are shared out.
#[cfg(feature = "physics3d")]
const CHARACTERS: usize = 32;

/// The tick the small scene's light is baked on (once, while profiling).
#[cfg(feature = "3d")]
const BAKE_AT: u64 = PROFILED_FROM + 5;

/// The spatial grid's entries: enough that its rebuild runs on all cores.
const GRID_ENTRIES: usize = 200_000;

/// The spatial grid, the flow field and the 3D frame are redone every this
/// many ticks.
const PASS_EVERY: u64 = 10;

/// The 3D models collected (and culled) every `PASS_EVERY` ticks.
#[cfg(all(feature = "3d", feature = "bench-internals"))]
const MODELS_3D: usize = 20_000;

/// Profiling is on: the scheduler runs batches on the page's thread.
static PROFILING: AtomicBool = AtomicBool::new(false);

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
    if !PROFILING.load(Ordering::Relaxed) {
        note(&SEEN_BATCH, &BATCH_THREADS);
    }
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
                "rumble": info.map(|i| i.capabilities.rumble),
                "trigger_rumble": info.map(|i| i.capabilities.trigger_rumble),
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
    /// The root the hierarchy's children follow.
    root: Option<Entity>,
    /// The hierarchy shared a level out.
    hier_shared: bool,
    /// The engine's own passes that went through the frame pool's runner
    /// (see `ENGINE_PASSES`).
    passes_shared: Vec<&'static str>,
    /// Rebuilt every `PASS_EVERY` ticks, big enough to go to the pool.
    grid: Option<ponko::spatial::SpatialGrid>,
    /// Computed every `PASS_EVERY` ticks toward a goal that moves.
    flow: Option<(ponko::flow::NavGrid, ponko::flow::FlowField)>,
    /// A world of 3D models, its frame collected every `PASS_EVERY` ticks.
    #[cfg(all(feature = "3d", feature = "bench-internals"))]
    scene3d: Option<Scene3D>,
    /// The bytes and a hash of the last JSON save of the game's world.
    save: Option<(usize, u64)>,
    /// What the 3D collection found at tick 120, with the checksum.
    frame3d: Option<(usize, usize, usize)>,
    /// Each timed pass's times in milliseconds (see `PASS_TIMES`).
    pass_ms: Vec<(&'static str, Vec<f64>)>,
    /// The vertices and batches of the last skeleton drawing.
    skeletons: (usize, usize),
    /// A hash of the baked lighting's bytes.
    bake_hash: Option<u64>,
}

impl Check {
    /// Records `ms` for the pass `name`.
    fn time(&mut self, name: &'static str, ms: f64) {
        match self.pass_ms.iter_mut().find(|(n, _)| *n == name) {
            Some((_, times)) => times.push(ms),
            None => self.pass_ms.push((name, vec![ms])),
        }
    }

    /// Each timed pass's median time, rounded to microseconds, in the
    /// order of `PASS_TIMES`.
    fn pass_medians(&self) -> serde_json::Map<String, serde_json::Value> {
        PASS_TIMES
            .iter()
            .filter_map(|name| self.pass_ms.iter().find(|(n, _)| n == name))
            .map(|(name, times)| {
                let mut sorted = times.clone();
                sorted.sort_by(f64::total_cmp);
                let median = sorted[sorted.len() / 2];
                ((*name).to_string(), json!((median * 1000.0).round() / 1000.0))
            })
            .collect()
    }
}

/// Runs `f`, returning its result and its time in milliseconds.
fn timed_call<R>(f: impl FnOnce() -> R) -> (R, f64) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed().as_secs_f64() * 1000.0)
}

/// A world of `MODELS_3D` models on a grid, some bobbing, collected for a
/// camera that turns: the retained 3D culling compares every model each
/// time, on all cores.
#[cfg(all(feature = "3d", feature = "bench-internals"))]
struct Scene3D {
    world: World,
    models: Vec<Entity>,
    extraction: ponko::render::ExtractionBench,
    camera: ponko::d3::Camera,
    lighting: ponko::render::Lighting,
    /// What the last collection found: instances for the camera and the
    /// shadow views, and draw runs.
    found: (usize, usize, usize),
}

/// The asset group's file decoded with `with`, and what its decoder returns.
const GROUP_FILE: &str = "audio_mixer.json";
type Decoded = (ThreadId, usize);

fn check(game: Game) {
    let state = Rc::new(std::cell::RefCell::new(Check::default()));
    let page = std::thread::current().id();
    let panic_at = (param("smoke_case").as_deref() == Some("worker-panic")).then_some(30);
    let mut game = game.parallel_systems(true).parallel_threshold(Duration::ZERO);
    #[cfg(feature = "physics3d")]
    {
        game = game.physics3d(vec3(0.0, -9.81, 0.0));
    }
    // Four view systems with nothing in common: one batch.
    macro_rules! batch {
        ($($n:literal)*) => {$(
            game = game.add(Phase::AfterPhysics, heavy::<$n>.access(SystemAccess::new().write::<Val<$n>>()));
        )*};
    }
    batch!(0 1 2 3);
    let sim = state.clone();
    let hier = state.clone();
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
        if let Some(root) = s.root
            && let Some(t) = ctx.world.get_mut::<d2::Transform>(root)
        {
            t.position = vec2(320.0 + (tick % 64) as f32, 180.0);
            t.rotation = tick as f32 * 0.01;
        }
        // Every tick while scopes are timed, so a frame sees them.
        let timed = (PROFILED_FROM..PROFILED_FROM + 20).contains(&tick);
        if tick.is_multiple_of(PASS_EVERY) || timed {
            engine_passes(&mut s, tick, timed);
            #[cfg(feature = "bench-internals")]
            {
                let ((), ms) = timed_call(|| ctx.bench_skeleton2d_draw());
                s.skeletons = ctx.bench_skeleton2d_finish();
                if timed {
                    s.time("skeleton2d/shared_draw", ms);
                }
            }
        }
        if timed {
            let (save, ms) = timed_call(|| save_json(&ctx.world));
            s.save = Some(save);
            s.time("reflect/save", ms);
        }
        #[cfg(feature = "3d")]
        if tick == BAKE_AT {
            let (hash, ms) = timed_call(bake);
            s.bake_hash = Some(hash);
            s.time("three/shared_bake", ms);
        }
        #[cfg(feature = "physics3d")]
        walk_characters(ctx, tick);
        if tick == 120 {
            let mut sum = 0.0f64;
            ctx.world
                .query::<&Pos>()
                .for_each(|p| sum += f64::from(p.x) + f64::from(p.y));
            ctx.world
                .query::<(&d2::Transform, &Attached)>()
                .for_each(|(t, _)| sum += f64::from(t.position.x) + f64::from(t.position.y));
            sum += passes_checksum(&s);
            #[cfg(feature = "physics3d")]
            {
                sum += characters_checksum(&mut ctx.world);
            }
            s.checksum = Some(sum);
            s.frame3d = frame3d(&s);
        }
        if tick.is_multiple_of(30) && s.job.is_none() {
            s.job = Some(Job::spawn(|| std::thread::current().id()));
        }
    })
    // The hierarchy's update: the children follow the root.
    .sim_system(move |ctx| {
        let ((), ms) = timed_call(|| ponko::systems::follow_attachments(ctx));
        let mut s = hier.borrow_mut();
        if (PROFILED_FROM..PROFILED_FROM + 20).contains(&s.ticks) {
            s.time("hierarchy/shared_level", ms);
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
        // Scopes are timed for a few ticks (the scheduler doesn't batch
        // while profiling): `hierarchy/shared_level` runs once per level
        // shared out.
        let profiling = (PROFILED_FROM..PROFILED_FROM + 20).contains(&s.ticks);
        ctx.perf.set_profiling(profiling);
        PROFILING.store(profiling, Ordering::Relaxed);
        s.hier_shared |= ctx
            .perf
            .scope_timings()
            .iter()
            .any(|t| t.name == "hierarchy/shared_level" && t.calls > 0);
        let mut characters_ms = None;
        for t in ctx.perf.scope_timings() {
            if t.calls > 0 && ENGINE_PASSES.contains(&t.name) && !s.passes_shared.contains(&t.name) {
                s.passes_shared.push(t.name);
            }
            // Inside the physics step, so timed by its scope (per step),
            // from whichever frame's timings hold it.
            if t.calls > 0 && t.name == "physics3d/shared_characters" {
                characters_ms = Some(f64::from(t.last_ms) / f64::from(t.calls));
            }
        }
        if let Some(ms) = characters_ms {
            s.time("physics3d/shared_characters", ms);
        }
        let batch_threaded_runs = s.batch_threaded_runs;
        s.pad_presses += ctx
            .input
            .gamepads()
            .iter()
            .filter(|&&p| ctx.input.gamepad_pressed(p, GamepadButton::South))
            .count() as u64;
        let west: Vec<_> = ctx
            .input
            .gamepads()
            .iter()
            .copied()
            .filter(|&p| ctx.input.gamepad_pressed(p, GamepadButton::West))
            .collect();
        for pad in west {
            ctx.input.rumble(pad, Rumble::new(0.8, 0.3).seconds(2.5));
        }
        let north: Vec<_> = ctx
            .input
            .gamepads()
            .iter()
            .copied()
            .filter(|&p| ctx.input.gamepad_pressed(p, GamepadButton::North))
            .collect();
        for pad in north {
            ctx.input
                .rumble(pad, Rumble::new(0.0, 0.2).triggers(0.4, 0.9).seconds(1.5));
        }
        let extra = json!({
            "threads": engine_threads(),
            "isolated": isolated(),
            "par_threads": par_threads,
            "par_on_page": par_on_page,
            "batch_threads": batch_threads,
            "batch_on_page": batch_on_page,
            "batch_threaded_runs": batch_threaded_runs,
            "hier_shared": s.hier_shared,
            "passes_shared": s.passes_shared,
            "pass_ms": s.pass_medians(),
            "frame3d": s.frame3d.map(|(visible, shadow, runs)| [visible, shadow, runs]),
            "save_bytes": s.save.map(|(bytes, _)| bytes),
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
        // Tracked transforms: the hierarchy places the children by lookups.
        ctx.world.track_changes::<d2::Transform>();
        let root = ctx.world.spawn((d2::Transform::at(vec2(320.0, 180.0)),));
        for i in 0..CHILDREN {
            let f = i as f32;
            ctx.world.spawn((
                d2::Transform::default(),
                Attached::to(root).offset(vec2((f * 0.37) % 200.0, (f * 0.11) % 120.0)),
            ));
        }
        // Waited for on the page's thread, which spins (it may not block).
        let thread = Job::spawn(|| std::thread::current().id()).wait();
        // The children's transforms are saved as JSON (see `save_json`).
        ctx.world.reflect::<d2::Transform>();
        #[cfg(feature = "bench-internals")]
        spawn_skeletons(ctx);
        #[cfg(feature = "physics3d")]
        spawn_characters(ctx);
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
        s.root = Some(root);
    })
    .run();
}

/// The engine's own parallel passes: a spatial grid of `GRID_ENTRIES`
/// rebuilt, and a flow field computed toward a goal that moves with `tick`;
/// each timed when `timed`.
fn engine_passes(s: &mut Check, tick: u64, timed: bool) {
    let shift = (tick % 97) as f32;
    let grid = s.grid.get_or_insert_with(|| ponko::spatial::SpatialGrid::new(16.0));
    let ((), grid_ms) = timed_call(|| {
        grid.rebuild_from((0..GRID_ENTRIES as u64).map(|i| {
            let f = i as f32;
            (
                Entity::from_bits(i + 1),
                vec2((f * 7.31 + shift) % 4096.0, (f * 3.17 + shift * 0.5) % 4096.0),
            )
        }));
    });
    let (nav, flow) = s.flow.get_or_insert_with(|| {
        let mut nav = ponko::flow::NavGrid::new(256, 256, 4.0);
        for y in 20..236 {
            nav.set_cost(128, y, ponko::flow::NavGrid::WALL);
        }
        (nav, ponko::flow::FlowField::new())
    });
    let ((), flow_ms) = timed_call(|| flow.compute(nav, &[vec2(8.0 + shift * 4.0, 512.0)]));
    #[cfg(all(feature = "3d", feature = "bench-internals"))]
    let cull_ms = {
        let scene = s.scene3d.get_or_insert_with(scene_3d);
        timed_call(|| collect_3d(scene, tick)).1
    };
    if timed {
        s.time("spatial/shared_rebuild", grid_ms);
        s.time("flow/shared_directions", flow_ms);
        #[cfg(all(feature = "3d", feature = "bench-internals"))]
        s.time("three/shared_cull", cull_ms);
    }
}

/// `SKELETONS` 2D skeletons of a bone and a sprite each, spread over the
/// field: drawn by the game every frame, and by the fixture (timed) every
/// `PASS_EVERY` ticks.
#[cfg(feature = "bench-internals")]
fn spawn_skeletons(ctx: &mut Ctx) {
    use ponko::skeleton2d::{BonePose, Skeleton, SkeletonData, SpritePart};
    let pixel = ctx.gfx.create_texture(1, 1, &[255; 4]);
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let arm = data.add_bone("arm", Some(root), BonePose::at(vec2(4.0, 0.0)), 8.0);
    for (i, bone) in [root, arm].into_iter().enumerate() {
        let slot = data.add_slot(&format!("slot{i}"), bone);
        data.add_attachment(slot, "part", SpritePart::new(pixel).with_size(vec2(4.0, 4.0)));
    }
    let data = std::sync::Arc::new(data);
    for i in 0..SKELETONS {
        let f = i as f32;
        ctx.world.spawn((
            d2::Transform::at(vec2((f * 13.7) % 640.0, (f * 7.9) % 360.0)),
            Skeleton::new(data.clone()),
        ));
    }
}

/// `CHARACTERS` 3D characters on a floor, walking in circles: their moves
/// are planned on the frame pool every tick.
#[cfg(feature = "physics3d")]
fn spawn_characters(ctx: &mut Ctx) {
    ctx.world.spawn((
        ponko::d3::Transform::at(vec3(0.0, -0.5, 0.0)),
        ponko::d3::RigidBody::fixed(),
        ponko::d3::Collider::cuboid(vec3(200.0, 1.0, 200.0)),
    ));
    for i in 0..CHARACTERS {
        let (x, z) = ((i % 8) as f32 * 3.0 - 12.0, (i / 8) as f32 * 3.0 - 6.0);
        let mut c = ponko::d3::Character::new(1.8, 0.3 + (i % 7) as f32 * 0.001);
        c.push_mass = 0.0;
        ctx.world.spawn((ponko::d3::Transform::at(vec3(x, 1.0, z)), c));
    }
}

/// Steers every character along its circle.
#[cfg(feature = "physics3d")]
fn walk_characters(ctx: &mut Ctx, tick: u64) {
    let t = tick as f32 * 0.02;
    ctx.world.query::<&mut ponko::d3::Character>().for_each(|c| {
        let a = t + c.radius * 100.0;
        c.velocity.x = 3.0 * a.cos();
        c.velocity.z = 3.0 * a.sin();
    });
}

/// The characters' places summed, for the checksum.
#[cfg(feature = "physics3d")]
fn characters_checksum(world: &mut World) -> f64 {
    let mut sum = 0.0f64;
    world
        .query::<(&ponko::d3::Transform, &ponko::d3::Character)>()
        .for_each(|(t, _)| sum += f64::from(t.position.x + t.position.y * 3.0 + t.position.z * 5.0));
    sum
}

/// Bakes a small scene's light (a floor, a ball and a wall under the sky
/// and the sun) and returns an FNV-1a hash of the result's bytes, the same
/// however many threads baked it.
#[cfg(feature = "3d")]
fn bake() -> u64 {
    use ponko::render::{BakeModel, LightBakeScene, LightBakeSettings, Lighting, MeshData, Sky, bake_lighting};
    let mut scene = LightBakeScene::new(&Lighting::default(), &Sky::default());
    let tpu = 8.0;
    let mut floor = MeshData::plane(12.0);
    floor.generate_lightmap_uvs(tpu);
    scene.add(BakeModel::new(&floor, Mat4::IDENTITY));
    let mut ball = MeshData::sphere(1.0, 16);
    ball.generate_lightmap_uvs(tpu);
    scene.add(BakeModel::new(&ball, Mat4::from_translation(vec3(0.0, 1.0, 0.0))));
    let mut wall = MeshData::cube(1.0);
    wall.generate_lightmap_uvs(tpu);
    scene.add(BakeModel::new(
        &wall,
        Mat4::from_scale_rotation_translation(vec3(6.0, 3.0, 0.5), Quat::IDENTITY, vec3(0.0, 1.5, -3.0)),
    ));
    let settings = LightBakeSettings::default()
        .with_texels_per_unit(tpu)
        .with_samples(16)
        .with_page_size(256)
        .with_probe_spacing(2.0);
    let bytes = bake_lighting(&scene, &settings).map(|b| b.to_bytes()).unwrap_or_default();
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// What the last 3D collection found: `None` before the first or without
/// the 3D features.
fn frame3d(_s: &Check) -> Option<(usize, usize, usize)> {
    #[cfg(all(feature = "3d", feature = "bench-internals"))]
    if let Some(scene) = &_s.scene3d {
        return Some(scene.found);
    }
    None
}

/// `MODELS_3D` models on a grid, each of one of 16 unit meshes.
#[cfg(all(feature = "3d", feature = "bench-internals"))]
fn scene_3d() -> Scene3D {
    let extraction = ponko::render::ExtractionBench::new(16);
    let mut world = World::new();
    let models = (0..MODELS_3D)
        .map(|i| {
            let at = vec3((i % 200) as f32 * 4.0 - 400.0, 0.0, (i / 200) as f32 * 4.0 - 200.0);
            let mesh = extraction.mesh((i as u32).wrapping_mul(2_654_435_761) % 16);
            world.spawn((ponko::d3::Transform::at(at), ponko::render::Model::new(mesh)))
        })
        .collect();
    let mut camera = ponko::render::ExtractionBench::camera(1280.0, 720.0);
    camera.far = 300.0;
    Scene3D {
        world,
        models,
        extraction,
        camera,
        lighting: ponko::render::Lighting::default(),
        found: (0, 0, 0),
    }
}

/// Turns the camera and bobs every 50th model with `tick`, then collects
/// the frame: the models are compared and culled again.
#[cfg(all(feature = "3d", feature = "bench-internals"))]
fn collect_3d(scene: &mut Scene3D, tick: u64) {
    let angle = tick as f32 * 0.02;
    scene.camera.position = vec3(angle.cos() * 80.0, 12.0, angle.sin() * 80.0);
    scene
        .camera
        .look_at(vec3((angle + 0.6).cos() * 120.0, 0.0, (angle + 0.6).sin() * 120.0));
    let y = (tick % 3) as f32 * 0.25;
    for &e in scene.models.iter().step_by(50) {
        if let Some(t) = scene.world.get_mut::<ponko::d3::Transform>(e) {
            t.position.y = y;
        }
    }
    let sample = scene
        .extraction
        .collect(&mut scene.world, &scene.camera, &scene.lighting);
    scene.found = (sample.visible, sample.shadow, sample.runs);
}

/// The game's world saved as JSON (`World::save`, which builds each
/// entity's JSON on the frame pool): its length and an FNV-1a hash of the
/// text, the same however many threads built it.
fn save_json(world: &World) -> (usize, u64) {
    let text = serde_json::to_string(&world.save()).unwrap_or_default();
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    (text.len(), hash)
}

/// Sums where the grid's entries landed (in its order, which the rebuild
/// sets) and the flow field's directions and distances at sample points.
fn passes_checksum(s: &Check) -> f64 {
    let mut sum = 0.0f64;
    if let Some(grid) = &s.grid {
        for (k, e) in grid.entries().iter().enumerate().step_by(997) {
            sum += (k % 7) as f64 * f64::from(e.position.x + e.position.y);
        }
        sum += grid.count_in_radius(vec2(2048.0, 2048.0), 300.0) as f64;
    }
    if let Some((_, flow)) = &s.flow {
        for y in (2..1024).step_by(37) {
            for x in (2..1024).step_by(41) {
                let at = vec2(x as f32, y as f32);
                let d = flow.direction_at(at);
                sum += f64::from(d.x * 3.0 + d.y) + f64::from(flow.distance_at(at).unwrap_or(-1.0));
            }
        }
    }
    #[cfg(all(feature = "3d", feature = "bench-internals"))]
    if let Some(scene) = &s.scene3d {
        let (visible, shadow, runs) = scene.found;
        sum += (visible * 3 + shadow * 5 + runs * 7) as f64;
    }
    if let Some((bytes, hash)) = s.save {
        sum += bytes as f64 + (hash % 1_000_003) as f64;
    }
    let (vertices, batches) = s.skeletons;
    sum += (vertices * 3 + batches * 5) as f64;
    if let Some(hash) = s.bake_hash {
        sum += (hash % 1_000_003) as f64;
    }
    sum
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
