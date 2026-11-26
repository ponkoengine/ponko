//! The game playing itself: the packaged-game smoke test, the full
//! playthrough test, and the web build's state report.
//!
//! `ember_hop --smoke headless|window` starts a run, plays both levels with
//! [`autopilot`] (hold right; jump at walls and gaps; flare at drips),
//! leaves for the title once in the second level and continues the saved
//! run, reaches the end screen and goes back to the title. It writes a JSON
//! [`Report`] of what it saw and exits with 0 only if it got there.

use super::*;
use std::path::PathBuf;

/// What happened, written as JSON when the walk-through ends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    /// `headless` or `window`.
    pub mode: String,
    /// The screens in the order the game showed them.
    pub screens: Vec<String>,
    pub levels_cleared: u32,
    pub sparks: u32,
    pub deaths: u32,
    pub jumps: u32,
    pub flares: u32,
    pub drips_burned: u32,
    pub saves: u32,
    pub loads: u32,
    /// How the progress file was found at startup: `Fresh`, `Loaded`, `Recovered(..)`.
    pub progress_at_start: String,
    pub runs_finished: u32,
    /// The folder `ctx.storage` kept the files in (`None`: none kept, or the web).
    pub storage_dir: Option<String>,
    /// The hero's picture, and its size once loaded (the packaged game found it).
    pub asset: String,
    pub asset_size: [u32; 2],
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
    since: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// On the title: start a run.
    Start,
    /// Play the first level.
    PlayFirst,
    /// In the second level for a moment: pause.
    Pause,
    /// Paused: leave for the title (the run stays saved).
    Leave,
    /// On the title: continue the saved run.
    Continue,
    /// Play to the end screen.
    PlayRest,
    /// On the end screen: any key.
    Done,
    /// Back on the title: report.
    Report,
}

/// Ticks on a screen before acting.
const DWELL: u64 = 20;

/// The walk-through gives up after this many ticks (four minutes of play),
/// reporting `finished: false`.
pub const MAX_TICKS: u64 = 4 * 60 * 60;

pub fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::Title => "Title",
        Screen::Playing => "Playing",
        Screen::Paused => "Paused",
        Screen::Dying => "Dying",
        Screen::Transition { .. } => "Transition",
        Screen::Finished => "Finished",
    }
}

/// Startup for the walk-through, after the game's own.
pub fn setup_smoke(ctx: &mut Ctx, report_path: Option<PathBuf>, mode: &str) {
    let hero = ctx.world.resource::<Art>().hero.texture;
    let report = Report {
        mode: mode.to_string(),
        progress_at_start: format!("{:?}", ctx.world.resource::<LoadStatus>()),
        storage_dir: ctx.storage.dir().map(|d| d.display().to_string()),
        asset: HERO_PNG.to_string(),
        asset_size: [hero.width(), hero.height()],
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
        since: 0,
    });
}

impl Smoke {
    pub fn finished(&self) -> bool {
        self.report.finished
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
}

/// Sim system (before the rules): the walk-through's player.
pub fn walkthrough(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let screen = screen(&ctx.world);
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
    let level = ctx.world.resource::<Run>().level;
    if looping && screen == Screen::Dying && !ctx.world.resource::<Smoke>().death_attempted {
        let smoke = ctx.world.resource_mut::<Smoke>();
        smoke.death_attempted = true;
        smoke.deaths += 1;
    }
    let mut next = step;
    match step {
        Step::Start if screen == Screen::Title && waited => {
            ctx.world.resource_mut::<Intent>().confirm = true;
            next = Step::PlayFirst;
        }
        Step::PlayFirst if screen == Screen::Playing && level == 1 => next = Step::Pause,
        Step::PlayFirst => {
            if looping && waited && screen == Screen::Playing && !ctx.world.resource::<Smoke>().death_attempted {
                let hero = ctx
                    .world
                    .query::<(Entity, &Player)>()
                    .iter()
                    .next()
                    .map(|(entity, _)| entity);
                if let Some(hero) = hero {
                    lose_hero(ctx, hero);
                }
            } else {
                autopilot(ctx);
            }
        }
        Step::Pause if waited => {
            ctx.world.resource_mut::<Intent>().pause = true;
            next = Step::Leave;
        }
        Step::Leave if screen == Screen::Paused && waited => {
            ctx.world.resource_mut::<Intent>().back = true;
            next = Step::Continue;
        }
        Step::Continue if screen == Screen::Title && waited => {
            ctx.world.resource_mut::<Intent>().resume_run = true;
            next = Step::PlayRest;
        }
        Step::PlayRest if screen == Screen::Finished => next = Step::Done,
        Step::PlayRest => autopilot(ctx),
        Step::Done if waited => {
            ctx.world.resource_mut::<Intent>().any = true;
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

// ANCHOR: autopilot
/// A simple player for left-to-right levels: run right, flare at drips
/// close ahead, jump at walls and gaps, and hold the jump to its top.
pub fn autopilot(ctx: &mut Ctx) {
    let mut intent = Intent {
        move_x: 1.0,
        ..*ctx.world.resource::<Intent>()
    };
    if screen(&ctx.world) != Screen::Playing {
        return;
    }
    let Some((hero, at, grounded)) = ctx
        .world
        .query::<(Entity, &Transform, &Player)>()
        .iter()
        .next()
        .map(|(e, t, p)| (e, t.position, p.grounded))
    else {
        return;
    };
    // Close enough that the flare's circle surely reaches it.
    let drip_near = ctx
        .world
        .query::<(&Transform, &Drip)>()
        .iter()
        .any(|(t, _)| t.position.distance(at) < FLARE_RADIUS - 2.0);
    if drip_near {
        intent.flare = true;
    }
    let filter = QueryFilter::default().exclude(hero);
    let feet = at.y + HERO_OFFSET.y + HERO_SIZE.y / 2.0;
    let wall = [feet - 3.0, at.y].iter().any(|&y| {
        ctx.physics
            .cast_ray(vec2(at.x, y), vec2(1.0, 0.0), 10.0, filter)
            .is_some()
    });
    let gap = ctx
        .physics
        .cast_ray(vec2(at.x + 9.0, feet - 2.0), vec2(0.0, 1.0), 20.0, filter)
        .is_none();
    let mut hold = ctx.world.get_resource::<AutopilotHold>().map_or(0, |h| h.0);
    if grounded && (wall || gap) {
        intent.jump = true;
        hold = 20;
    }
    intent.jump_held = hold > 0;
    hold = hold.saturating_sub(1);
    ctx.world.insert_resource(AutopilotHold(hold));
    *ctx.world.resource_mut::<Intent>() = intent;
}
// ANCHOR_END: autopilot

/// Ticks left for the autopilot to hold the jump button.
pub struct AutopilotHold(pub u32);

/// Frame system: counts the frames drawn, for the report.
pub fn count_frames(ctx: &mut Ctx) {
    if let Some(smoke) = ctx.world.get_resource_mut::<Smoke>() {
        smoke.report.frames += 1;
    }
}

fn finish(ctx: &mut Ctx, tick: u64, completed: bool) {
    let tally = ctx.world.resource::<Tally>().clone();
    let progress = ctx.world.resource::<Progress>().clone();
    let run = ctx.world.resource::<Run>().clone();
    if completed && ctx.world.resource::<Smoke>().looping {
        let smoke = ctx.world.resource_mut::<Smoke>();
        smoke.cycles += 1;
        smoke.cycle_start = tick;
        smoke.since = tick;
        smoke.step = Step::Start;
        smoke.death_attempted = false;
        return;
    }
    let smoke = ctx.world.resource_mut::<Smoke>();
    let report = &mut smoke.report;
    report.levels_cleared = tally.levels_cleared;
    report.sparks = run.sparks;
    report.deaths = run.deaths;
    report.jumps = tally.jumps;
    report.flares = tally.flares;
    report.drips_burned = tally.burned;
    report.saves = tally.saves;
    report.loads = tally.loads;
    report.runs_finished = progress.runs_finished;
    report.ticks = tick;
    report.finished = completed;
    let json = serde_json::to_string_pretty(&*report).expect("a report is plain data");
    match &smoke.report_path {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("ember_hop: could not write the report {}: {e}", path.display());
            }
        }
        None => println!("{json}"),
    }
    ctx.quit();
}

/// The game with the walk-through installed, its files where `game` keeps them.
pub fn smoke_game(game: Game) -> Game {
    game.seed(SEED)
        .sim_system(walkthrough)
        .plugin(EmberHop)
        .add(Phase::Frame, count_frames)
}

/// [`smoke_game`] with the files in the player's data folder, like the real game.
pub fn build_smoke() -> Game {
    smoke_game(Game::new("Ember Hop", WINDOW.x as u32, WINDOW.y as u32).app_id(APP_ID))
}

/// Frame system for the web build: the state on `<html data-smoke>`, in the
/// format of `examples/web_smoke.rs` plus the screen, for the browser runner
/// of `web/smoke` (and the web gallery).
#[cfg(target_arch = "wasm32")]
pub fn publish_web_state(ctx: &mut Ctx) {
    #[derive(Default)]
    struct Frames(u64);
    let mut frames = ctx.world.remove_resource::<Frames>().unwrap_or_default();
    frames.0 += 1;
    let hero = ctx.world.resource::<Art>().hero.texture;
    let size = ctx.window_size();
    let run = ctx.world.resource::<Run>();
    let state = format!(
        r#"{{"ticks":{},"frames":{},"asset":[{},{}],"size":[{},{}],"screen":"{}","level":{},"sparks":{},"storage":"{}","saving":{}}}"#,
        ctx.time.sim_tick(),
        frames.0,
        hero.width(),
        hero.height(),
        size.x,
        size.y,
        screen_name(screen(&ctx.world)),
        run.level,
        run.sparks,
        if ctx.storage.is_persistent() {
            "persistent"
        } else {
            "memory"
        },
        ctx.storage.is_saving(),
    );
    ctx.world.insert_resource(frames);
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-smoke", &state);
    }
}
