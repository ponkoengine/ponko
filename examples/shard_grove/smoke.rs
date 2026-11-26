//! The game playing itself: the packaged-game smoke test, the full
//! playthrough test, and the web build's state report.
//!
//! `shard_grove --smoke headless|window` starts a new game, walks to three
//! shards with [`autopilot`] (paths on the crawlers' navigation mesh, a
//! pulse whenever a crawler comes close), saves at a shrine, quits to the
//! title and continues the saved game, finds the other shards, leaves by
//! the gate and goes back to the title. It writes a JSON [`Report`] of what
//! it saw and exits with 0 only if it got there.

use super::*;
use std::path::PathBuf;

/// What happened, written as JSON when the walk-through ends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    /// `headless` or `window`.
    pub mode: String,
    /// The screens in the order the game showed them.
    pub screens: Vec<String>,
    pub tally: Tally,
    /// Seconds the winning run took.
    pub time: f32,
    /// The quality preset in use at the start.
    pub quality: String,
    /// The folder `ctx.storage` kept the files in (`None`: none kept, or the web).
    pub storage_dir: Option<String>,
    /// The warden's joints and the corners of the navigation path to the
    /// gate: the packaged game found and read its files.
    pub warden_joints: usize,
    pub path_corners: usize,
    /// Frames drawn (0 headless).
    pub frames: u64,
    pub ticks: u64,
    pub finished: bool,
}

/// The walk-through's progress. A resource.
pub struct Smoke {
    report_path: Option<PathBuf>,
    report: Report,
    step: Step,
    looping: bool,
    cycle_start: u64,
    /// Completed walk-throughs, with a fresh per-cycle timeout.
    pub cycles: u64,
    pub deaths: u64,
    death_attempted: bool,
    saves_at_start: u32,
    since: u64,
    /// The walk-through has ended, finished or not (the web build keeps
    /// running after it, showing the title).
    over: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// On the title: start a new game.
    Start,
    /// Find three shards.
    CollectFirst,
    /// Walk to a shrine until it saves.
    Save,
    /// Pause.
    Pause,
    /// Paused: quit to the title (the save stays).
    Leave,
    /// On the title: continue the saved game.
    Continue,
    /// Find the rest, then walk through the gate.
    Finish,
    /// On the end screen: back to the title.
    Done,
    /// Back on the title: report.
    Report,
}

/// Ticks on a screen before acting.
const DWELL: u64 = 20;
/// Shards found before the walk-through saves.
const FIRST_SHARDS: usize = 3;

/// The walk-through gives up after this many ticks (four minutes of play at
/// the 50 Hz of 3D physics), reporting `finished: false`.
pub const MAX_TICKS: u64 = 4 * 60 * 50;

pub fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::Title => "Title",
        Screen::Playing => "Playing",
        Screen::Paused => "Paused",
        Screen::Settings { .. } => "Settings",
        Screen::Won => "Won",
        Screen::Lost => "Lost",
    }
}

/// Startup for the walk-through, after the game's own.
pub fn setup_smoke(ctx: &mut Ctx, report_path: Option<PathBuf>, mode: &str) {
    let warden_joints = ctx.world.resource::<GroveAssets>().warden_skeleton.joints().len();
    let path_corners = ctx
        .world
        .resource::<NavMesh>()
        .find_path(START, GATE)
        .map_or(0, |p| p.len());
    let report = Report {
        mode: mode.to_string(),
        quality: quality_name(ctx).to_string(),
        storage_dir: ctx.storage.dir().map(|d| d.display().to_string()),
        warden_joints,
        path_corners,
        ..Report::default()
    };
    ctx.world.insert_resource(Smoke {
        report_path,
        report,
        step: Step::Start,
        looping: mode == "soak",
        cycle_start: 0,
        cycles: 0,
        deaths: 0,
        death_attempted: false,
        saves_at_start: 0,
        since: 0,
        over: false,
    });
}

impl Smoke {
    pub fn finished(&self) -> bool {
        self.report.finished
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    /// The walk-through ended, finished or given up.
    pub fn over(&self) -> bool {
        self.over
    }
    /// The step it is on, for the web build's state.
    pub fn step_name(&self) -> String {
        format!("{:?}", self.step)
    }
}

fn menu(ctx: &mut Ctx, choice: Menu) {
    ctx.world.resource_mut::<Intent>().menu = Some(choice);
}

/// Sim system (before the rules): the walk-through's player.
pub fn walkthrough(ctx: &mut Ctx) {
    if ctx.world.resource::<Smoke>().over {
        return;
    }
    let tick = ctx.time.sim_tick();
    let screen = *ctx.world.resource::<Screen>();
    let (step, since) = {
        let smoke = ctx.world.resource_mut::<Smoke>();
        if !smoke.looping {
            let name = screen_name(screen).to_string();
            if smoke.report.screens.last() != Some(&name) {
                smoke.report.screens.push(name);
            }
        }
        (smoke.step, smoke.since)
    };
    let (looping, cycle_start) = {
        let smoke = ctx.world.resource::<Smoke>();
        (smoke.looping, smoke.cycle_start)
    };
    if tick - cycle_start >= MAX_TICKS {
        finish(ctx, tick, false);
        return;
    }
    let waited = tick - since >= DWELL;
    let found = ctx.world.resource::<Run>().shards.len();
    let saves = ctx.world.resource::<Tally>().saves;
    let saves_at_start = ctx.world.resource::<Smoke>().saves_at_start;
    let mut next = step;
    // Lost: try again from the last save, wherever the walk-through was.
    if screen == Screen::Lost {
        if waited {
            menu(ctx, Menu::TryAgain);
            let smoke = ctx.world.resource_mut::<Smoke>();
            smoke.since = tick;
            if looping {
                smoke.deaths += 1;
            }
        }
        return;
    }
    match step {
        Step::Start if screen == Screen::Title && waited => {
            menu(ctx, Menu::NewGame);
            next = Step::CollectFirst;
        }
        Step::CollectFirst if found >= FIRST_SHARDS => next = Step::Save,
        Step::CollectFirst => autopilot(ctx, Goal::Shard),
        Step::Save if saves > saves_at_start => next = Step::Pause,
        Step::Save => autopilot(ctx, Goal::Shrine),
        Step::Pause if waited => {
            ctx.world.resource_mut::<Intent>().pause = true;
            next = Step::Leave;
        }
        Step::Leave if screen == Screen::Paused && waited => {
            menu(ctx, Menu::ToTitle);
            next = Step::Continue;
        }
        Step::Continue if screen == Screen::Title && waited => {
            menu(ctx, Menu::Continue);
            next = Step::Finish;
        }
        Step::Finish if screen == Screen::Won => next = Step::Done,
        Step::Finish if looping && screen == Screen::Playing && !ctx.world.resource::<Smoke>().death_attempted => {
            // Let the normal rules enter Lost and the existing driver reload.
            ctx.world.resource_mut::<Smoke>().death_attempted = true;
            ctx.world.resource_mut::<Run>().health = 0;
        }
        Step::Finish if found < SHARD_SPOTS.len() => autopilot(ctx, Goal::Shard),
        Step::Finish => autopilot(ctx, Goal::Gate),
        Step::Done if waited => {
            menu(ctx, Menu::Back);
            next = Step::Report;
        }
        Step::Report if screen == Screen::Title => {
            finish(ctx, tick, true);
            return;
        }
        _ => {}
    }
    if next != step {
        let smoke = ctx.world.resource_mut::<Smoke>();
        smoke.step = next;
        smoke.since = tick;
    }
}

/// Where the autopilot walks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    /// The closest shard not yet found.
    Shard,
    /// The closest shrine.
    Shrine,
    /// The gate.
    Gate,
}

// ANCHOR: autopilot
/// A simple player: follows a path on the navigation mesh to its goal,
/// turning it into the stick's direction relative to the view, and pulses
/// whenever a crawler that isn't stunned comes close.
pub fn autopilot(ctx: &mut Ctx, goal: Goal) {
    if *ctx.world.resource::<Screen>() != Screen::Playing {
        return;
    }
    let Some((_, at)) = warden(&mut ctx.world) else {
        return;
    };
    let target = match goal {
        Goal::Shard => ctx
            .world
            .query::<(&Transform, &Shard)>()
            .iter()
            .map(|(_, s)| SHARD_SPOTS[usize::from(s.id)])
            .min_by(|a, b| flat_distance(*a, at).total_cmp(&flat_distance(*b, at))),
        Goal::Shrine => SHRINE_SPOTS
            .into_iter()
            .min_by(|a, b| flat_distance(*a, at).total_cmp(&flat_distance(*b, at))),
        Goal::Gate => Some(GATE),
    };
    let Some(target) = target else {
        return;
    };
    let nav = ctx.world.resource::<NavMesh>();
    let from = nav.sample_position(at, 2.0).unwrap_or(at);
    let corner = nav
        .find_path(from, target)
        .and_then(|path| path.into_iter().find(|c| flat_distance(*c, at) > 0.3))
        .unwrap_or(target);
    let toward = vec3(corner.x - at.x, 0.0, corner.z - at.z).normalize_or_zero();
    // The stick is relative to the view: turn the world direction into it.
    let yaw = ctx.world.resource::<View>().yaw;
    let local = Quat::from_rotation_y(-yaw) * toward;
    let danger = ctx
        .world
        .query::<(&Transform, &Crawler)>()
        .iter()
        .any(|(t, c)| c.stun <= 0.0 && flat_distance(t.position, at) < 3.5);
    let intent = ctx.world.resource_mut::<Intent>();
    intent.movement = vec2(local.x, -local.z);
    intent.pulse |= danger;
}
// ANCHOR_END: autopilot

/// Frame system: counts the frames drawn, for the report.
pub fn count_frames(ctx: &mut Ctx) {
    if let Some(smoke) = ctx.world.get_resource_mut::<Smoke>() {
        smoke.report.frames += 1;
    }
}

fn finish(ctx: &mut Ctx, tick: u64, completed: bool) {
    let tally = ctx.world.resource::<Tally>().clone();
    let time = ctx.world.resource::<Records>().best_time.unwrap_or(0.0);
    if completed && ctx.world.resource::<Smoke>().looping {
        let smoke = ctx.world.resource_mut::<Smoke>();
        smoke.cycles += 1;
        smoke.cycle_start = tick;
        smoke.since = tick;
        smoke.step = Step::Start;
        smoke.death_attempted = false;
        smoke.saves_at_start = tally.saves;
        return;
    }
    let smoke = ctx.world.resource_mut::<Smoke>();
    let report = &mut smoke.report;
    report.tally = tally;
    report.time = time;
    report.ticks = tick;
    report.finished = completed;
    smoke.over = true;
    let json = serde_json::to_string_pretty(&*report).expect("a report is plain data");
    match &smoke.report_path {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("shard_grove: could not write the report {}: {e}", path.display());
            }
        }
        None => println!("{json}"),
    }
    // A page can't quit: the web build stays on the title and reports
    // `walkthrough_over` (see `publish_web_state`).
    if !cfg!(target_arch = "wasm32") {
        ctx.quit();
    }
}

/// The game with the walk-through installed, its files where `game` keeps them.
pub fn smoke_game(game: Game) -> Game {
    game.sim_system(walkthrough)
        .plugin(ShardGrove)
        .add(Phase::Frame, count_frames)
}

/// [`smoke_game`] with the files in the player's data folder, like the real game.
pub fn build_smoke() -> Game {
    smoke_game(Game::new(TITLE, WINDOW.0, WINDOW.1).app_id(APP_ID))
}

/// Asked for on the web build's address with `?smoke=walkthrough`: the
/// game plays the walk-through by itself, as `--smoke` does on the desktop.
#[cfg(target_arch = "wasm32")]
pub fn web_walkthrough_asked() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .any(|p| p == "smoke=walkthrough")
        })
}

/// Frame system for the web build: the state on `<html data-smoke>`, in the
/// format of `examples/web_smoke.rs` plus the screen, for the browser runner
/// of `web/smoke` (and the web gallery), and the engine's adapter report on
/// `<html data-smoke-gpu>` once. With the walk-through (`?smoke=walkthrough`)
/// it adds its step, whether it is over and finished, and the tally.
#[cfg(target_arch = "wasm32")]
pub fn publish_web_state(ctx: &mut Ctx) {
    #[derive(Default)]
    struct Published {
        frames: u64,
        clicks: u32,
        gpu: bool,
    }
    let mut published = ctx.world.remove_resource::<Published>().unwrap_or_default();
    published.frames += 1;
    if ctx.input.mouse_pressed(MouseButton::Left) {
        published.clicks += 1;
    }
    let document = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element());
    if !published.gpu
        && let Some(gpu) = ctx.gfx.gpu_capabilities()
    {
        if let Some(root) = &document {
            let _ = root.set_attribute("data-smoke-gpu", &serde_json::to_string(gpu).unwrap_or_default());
        }
        published.gpu = true;
    }
    let joints = ctx.world.resource::<GroveAssets>().warden_skeleton.joints().len();
    let size = ctx.window_size();
    let run = ctx.world.resource::<Run>();
    let tally = ctx.world.resource::<Tally>();
    let mut state = serde_json::json!({
        "ticks": ctx.time.sim_tick(),
        "frames": published.frames,
        "clicks": published.clicks,
        "joints": joints,
        "size": [size.x, size.y],
        "screen": screen_name(*ctx.world.resource::<Screen>()),
        // The menu button the keyboard is on (Enter presses it).
        "focus": ctx.ui.focus().and_then(|id| ctx.ui.name(id)),
        "shards": run.shards.len(),
        "tally": tally,
        "quality": quality_name(ctx),
        "storage": if ctx.storage.is_persistent() { "persistent" } else { "memory" },
    });
    if let Some(smoke) = ctx.world.get_resource::<Smoke>() {
        state["walkthrough"] = smoke.step_name().into();
        state["walkthrough_over"] = smoke.over().into();
        state["finished"] = smoke.finished().into();
    }
    ctx.world.insert_resource(published);
    if let Some(root) = document {
        let _ = root.set_attribute("data-smoke", &state.to_string());
    }
}
