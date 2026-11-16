//! Cookbook, "Sound and saves": footsteps, a sound for an event, saving and
//! loading a game, settings and a best score, and saving on quit. Every recipe
//! between `ANCHOR` lines is what `book/src/cookbook/sound-and-saves.md`
//! shows, and each test runs it headless for a few ticks and checks that it
//! did its job. A headless run opens no audio device, so nothing is heard,
//! but `ctx.audio.started()` lists what the game played, and
//! `ctx.quit_requested()` whether it asked to quit.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::audio::SoundStarted;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;
use ponko::storage::Storage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A storage folder of the test's own, in the system's temp folder: empty when made, removed when the
/// test ends, also when it ends by panicking. Nothing is ever written into the repository.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ponko-cookbook-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tap(scenario: InputScenario, key: Key, tick: u64) -> InputScenario {
    scenario
        .at(tick, InputEvent::Key(key, true))
        .at(tick + 1, InputEvent::Key(key, false))
}

// ANCHOR: footsteps
struct StepSound(Sound);

#[derive(Component)]
struct Stride {
    last: Vec2,  // where the last tick left it
    walked: f32, // distance since the last step
}

fn footsteps(ctx: &mut Ctx) {
    let step = ctx.world.resource::<StepSound>().0;
    let mut random = ctx.random().stream(ctx.time.sim_tick());
    ctx.world.query::<(&mut Stride, &Transform)>().for_each(|(stride, t)| {
        stride.walked += t.position.distance(stride.last);
        stride.last = t.position;
        if stride.walked >= 40.0 {
            stride.walked -= 40.0;
            // A little different each time, so it doesn't sound like a machine.
            let pitch = random.range(0.9, 1.1);
            ctx.audio.play_ex(
                step,
                SoundParams {
                    volume: 0.6,
                    speed: pitch,
                    looping: false,
                },
            );
        }
    });
}

fn load_step_sound(ctx: &mut Ctx) {
    // A short thump made in code; a game would `ctx.audio.load("step.ogg")` instead.
    let thump: Vec<f32> = (0..2000)
        .map(|i| (i as f32 * 0.05).sin() * (1.0 - i as f32 / 2000.0))
        .collect();
    let sound = ctx.audio.create(22_050, &thump);
    ctx.world.insert_resource(StepSound(sound));
}
// ANCHOR_END: footsteps

/// Every sound the game started, with the tick it started on, recorded by a
/// system that runs after the recipe's.
#[derive(Default)]
struct Played(Vec<(u64, SoundStarted)>);

fn record_played(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let started: Vec<_> = ctx.audio.started().iter().map(|s| (tick, *s)).collect();
    ctx.world.resource_mut::<Played>().0.extend(started);
}

#[test]
fn walking_two_hundred_pixels_plays_five_steps() {
    let world = Game::new("steps", 640, 480)
        .setup(|ctx| {
            load_step_sound(ctx);
            ctx.world.insert_resource(Played::default());
            ctx.world.spawn((
                Stride {
                    last: Vec2::ZERO,
                    walked: 0.0,
                },
                Transform::at(Vec2::ZERO),
            ));
        })
        .sim_system(|ctx: &mut Ctx| {
            // Walks 4 pixels a tick for 50 ticks.
            if ctx.time.sim_tick() < 50 {
                ctx.world.query::<&mut Transform>().for_each(|t| t.position.x += 4.0);
            }
        })
        .sim_system(footsteps)
        .sim_system(record_played)
        .run_headless(60);
    let played = &world.resource::<Played>().0;
    let ticks: Vec<u64> = played.iter().map(|(tick, _)| *tick).collect();
    assert_eq!(ticks, [9, 19, 29, 39, 49], "a step every 40 pixels");
    let step = world.resource::<StepSound>().0;
    for (_, s) in played {
        assert_eq!((s.sound, s.volume, s.looping), (step, 0.6, false));
        assert!((0.9..1.1).contains(&s.speed), "pitch {}", s.speed);
    }
    assert!(
        played.windows(2).any(|w| w[0].1.speed != w[1].1.speed),
        "not like a machine"
    );
}

// ANCHOR: sound_on_event
struct Picked; // an event, sent when the player picks something up

struct CoinSound {
    sound: Sound,
    quiet_until: f64, // simulation time
}

/// One sound per pickup, but never more than one every 50 ms: ten coins in
/// the same tick would only be louder, not better.
fn coin_sound(ctx: &mut Ctx) {
    let now = ctx.time.sim_time();
    if ctx.events.read::<Picked>().is_empty() {
        return;
    }
    let coin = ctx.world.resource_mut::<CoinSound>();
    if now >= coin.quiet_until {
        coin.quiet_until = now + 0.05;
        let sound = coin.sound;
        ctx.audio.play(sound);
    }
}
// ANCHOR_END: sound_on_event

#[test]
fn ten_pickups_in_a_tick_make_one_sound_and_a_later_pickup_another() {
    let world = Game::new("coin sound", 640, 480)
        .setup(|ctx| {
            let sound = ctx.audio.create(22_050, &[0.0; 1000]);
            ctx.world.insert_resource(CoinSound {
                sound,
                quiet_until: 0.0,
            });
            ctx.world.insert_resource(Played::default());
        })
        .sim_system(|ctx: &mut Ctx| {
            if [1, 2, 20].contains(&ctx.time.sim_tick()) {
                for _ in 0..10 {
                    ctx.events.send(Picked);
                }
            }
        })
        .sim_system(coin_sound)
        .sim_system(record_played)
        .run_headless(30);
    let coin = world.resource::<CoinSound>().sound;
    let played: Vec<(u64, Sound)> = world
        .resource::<Played>()
        .0
        .iter()
        .map(|(t, s)| (*t, s.sound))
        .collect();
    assert_eq!(
        played,
        [(1, coin), (20, coin)],
        "tick 2 was inside the 50 ms; tick 20 was not"
    );
}

// ANCHOR: save_game
#[derive(Component, Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
struct Health(i32);

fn register_saved_components(ctx: &mut Ctx) {
    ctx.world.reflect::<Health>(); // only reflected components go in a save
}

/// F5 saves, F9 loads. A slot is a name: "quick", "slot 1", "autosave"...
fn quick_save_and_load(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::F5)
        && let Err(e) = ctx.storage.save_world(&ctx.world, "quick")
    {
        eprintln!("could not save: {e}"); // a full disk shouldn't stop the game
    }
    if ctx.input.key_pressed(Key::F9) && ctx.load_world("quick").is_err() {
        eprintln!("no quick save yet"); // the world stays as it was
    }
}
// ANCHOR_END: save_game

#[test]
fn f5_saves_the_world_and_f9_brings_it_back() {
    let dir = TempDir::new("quick");
    let scenario = tap(tap(tap(InputScenario::new(), Key::F9, 1), Key::F5, 3), Key::F9, 8);
    let mut world = Game::new("saves", 640, 480)
        .app_id("cookbook-saves")
        .data_dir(&dir.0)
        .setup(|ctx| {
            register_saved_components(ctx);
            ctx.world.spawn((Health(7),));
        })
        .sim_system(quick_save_and_load)
        .sim_system(|ctx: &mut Ctx| {
            // Tick 5: the player gets hurt after saving.
            if ctx.time.sim_tick() == 5 {
                ctx.world.query::<&mut Health>().for_each(|h| h.0 = 1);
            }
        })
        .input_scenario(scenario)
        .run_headless(12);
    // F9 on tick 1 found no save and changed nothing; the one on tick 8 restored 7.
    assert_eq!(
        world.query::<&Health>().iter().copied().collect::<Vec<_>>(),
        [Health(7)]
    );
    assert!(dir.0.join("saves").join("quick.json").is_file());
}

// ANCHOR: settings
/// What the player chose, and their best score. A new field gets its default
/// when an old file lacks it, so an update doesn't lose anyone's settings.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
struct Progress {
    volume: f32,
    best_score: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            volume: 0.8,
            best_score: 0,
        }
    }
}

fn load_progress(ctx: &mut Ctx) {
    // No file on the first run, or one that is damaged: start from the defaults.
    let progress: Progress = ctx.storage.read_json("progress.json").unwrap_or_default();
    ctx.world.insert_resource(progress);
}

/// Call when a round ends. The write is safe: a crash can't leave half a file.
fn finish_round(ctx: &mut Ctx, score: u32) {
    let progress = ctx.world.resource_mut::<Progress>();
    if score > progress.best_score {
        progress.best_score = score;
        let _ = ctx
            .storage
            .write_json("progress.json", ctx.world.resource::<Progress>());
    }
}
// ANCHOR_END: settings

#[test]
fn the_best_score_is_kept_between_runs_and_a_lower_one_does_not_replace_it() {
    let dir = TempDir::new("progress");
    let run = |score: u32| {
        Game::new("progress", 640, 480)
            .app_id("cookbook-progress")
            .data_dir(&dir.0)
            .setup(load_progress)
            .setup(move |ctx| finish_round(ctx, score))
            .run_headless(1)
    };
    assert_eq!(run(120).resource::<Progress>().best_score, 120);
    assert_eq!(
        run(80).resource::<Progress>().best_score,
        120,
        "the file said 120 at start-up"
    );
    assert_eq!(run(200).resource::<Progress>().best_score, 200);
    let after = run(1);
    assert_eq!(
        *after.resource::<Progress>(),
        Progress {
            volume: 0.8,
            best_score: 200
        }
    );
}

// ANCHOR: save_on_quit
/// The menu's "Save and quit". Saves first, and quits only if that worked,
/// so a failed save is never lost along with the game.
fn save_and_quit(ctx: &mut Ctx) {
    if !ctx.input.key_pressed(Key::Q) {
        return;
    }
    match ctx.storage.save_world(&ctx.world, "autosave") {
        Ok(()) => ctx.quit(),
        Err(e) => eprintln!("could not save, staying in the game: {e}"),
    }
}
// ANCHOR_END: save_on_quit

#[test]
fn quit_writes_the_save_first_and_stops_the_game() {
    let dir = TempDir::new("quit");
    let world = Game::new("quit", 640, 480)
        .app_id("cookbook-quit")
        .data_dir(&dir.0)
        .setup(|ctx| {
            register_saved_components(ctx);
            ctx.world.spawn((Health(3),));
            ctx.world.insert_resource(Vec::<(u64, bool)>::new());
        })
        .sim_system(save_and_quit)
        .sim_system(|ctx: &mut Ctx| {
            let entry = (ctx.time.sim_tick(), ctx.quit_requested());
            ctx.world.resource_mut::<Vec<(u64, bool)>>().push(entry);
        })
        .input_scenario(tap(InputScenario::new(), Key::Q, 4))
        .run_headless(50);
    let seen = world.resource::<Vec<(u64, bool)>>();
    assert_eq!(
        seen.last(),
        Some(&(4, true)),
        "Q on tick 4 asked to quit, and no tick ran after it"
    );
    assert!(seen[..4].iter().all(|(_, quit)| !quit));
    let mut files = Storage::in_folder(&dir.0);
    let slots = files.world_slots().unwrap();
    assert_eq!(slots, ["autosave"]);
    let _ = files.delete_world("autosave");
}
