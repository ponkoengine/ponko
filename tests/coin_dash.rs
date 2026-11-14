//! The `coin_dash` reference game, played headlessly: the title screen, rounds,
//! pause, the result screen, saved progress (including a missing or damaged
//! save file) and the input mapping, with no window and no GPU. The tests drive
//! the game's own systems (`examples/coin_dash/game.rs`) with a scripted player
//! or with scheduled raw key presses.

#[path = "../examples/coin_dash/game.rs"]
mod game;

use game::*;
#[allow(unused_imports)]
use ponko::actions::InputEvent;
#[allow(unused_imports)]
use ponko::d2::Transform as Transform2D;
#[allow(unused_imports)]
use ponko::input::{Gamepad, GamepadAxis, GamepadButton, Input};
use ponko::prelude::*;
#[allow(unused_imports)]
use ponko::scenario::InputScenario;
#[allow(unused_imports)]
use ponko::ui::UiInput;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

/// The sim runs at the default 60 Hz: ticks per second, for readable run lengths.
const HZ: u64 = 60;

// ANCHOR: script
/// A system that runs before the game's rules and decides what the player
/// does this tick (in place of `read_input`, which needs a window's frames).
fn script(f: impl FnMut(&mut Ctx) + 'static) -> Game {
    script_saving(None, f)
}

/// [`script`] with progress kept at `save` (the game's storage is that
/// file's folder, and the settings go beside it). The game's plugin is added
/// unchanged, after the scripted player.
fn script_saving(save: Option<PathBuf>, f: impl FnMut(&mut Ctx) + 'static) -> Game {
    let game = Game::new("coin dash test", 800, 600);
    let game = match save.as_deref() {
        Some(path) => game.data_dir(folder_of(path)),
        None => game,
    };
    game.seed(SEED).sim_system(f).plugin(CoinDash)
}

/// The storage folder of a progress file at `path`.
fn folder_of(path: &std::path::Path) -> PathBuf {
    path.parent().expect("a progress file in a folder").to_path_buf()
}

/// Storage holding the progress file at `path`, as the game sees it.
fn storage_of(path: &std::path::Path) -> ponko::storage::Storage {
    ponko::storage::Storage::in_folder(folder_of(path))
}
// ANCHOR_END: script

/// For a [`script`]ed game: nothing more at startup, so it begins on the
/// title screen.
fn on_title(_: &mut Ctx) {}

/// For a [`script`]ed game: skips the title screen, round 1 is already running.
fn first_round(ctx: &mut Ctx) {
    new_round(ctx, 1);
}

/// The game's startup alone, for tests that call its systems by hand in a
/// game without them: every layer's `setup`, in the plugin's order.
fn setup_with(ctx: &mut Ctx, save: Option<PathBuf>) {
    if let Some(path) = save {
        ctx.storage = storage_of(&path);
    }
    PlayerInput.setup(ctx);
    SettingsScreen.setup(ctx);
    Rules.setup(ctx);
    Drawing.setup(ctx);
}

/// [`setup_with`] nothing saved, then round 1 already running.
fn setup_playing(ctx: &mut Ctx) {
    setup_with(ctx, None);
    new_round(ctx, 1);
}

fn intent(ctx: &mut Ctx) -> &mut Intent {
    ctx.world.resource_mut::<Intent>()
}

fn screen(world: &World) -> Screen {
    *world.resource::<Screen>()
}

fn player_and_coins(ctx: &mut Ctx) -> (Vec2, Vec<Vec2>) {
    let mut player = Vec2::ZERO;
    for (t, _) in ctx.world.query::<(&Transform2D, &Player)>().iter() {
        player = t.position;
    }
    let coins = ctx
        .world
        .query::<(&Transform2D, &Coin)>()
        .iter()
        .map(|(t, _)| t.position)
        .collect();
    (player, coins)
}

// ANCHOR: chase
/// Walks straight to the nearest coin.
fn chase_nearest_coin(ctx: &mut Ctx) {
    let (player, coins) = player_and_coins(ctx);
    let nearest = coins
        .into_iter()
        .min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)));
    let direction = nearest.map_or(Vec2::ZERO, |c| (c - player).normalize_or_zero());
    ctx.world.resource_mut::<Intent>().direction = direction;
}
// ANCHOR_END: chase

fn coin_layout(world: &mut World) -> Vec<Vec2> {
    let mut coins: Vec<Vec2> = world
        .query::<(&Transform2D, &Coin)>()
        .iter()
        .map(|(t, _)| t.position)
        .collect();
    coins.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    coins
}

/// A fresh, empty folder for one test's save file (under the system temp dir).
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-coin-dash-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Plays round 1 to a win (chasing coins) with progress kept in `save`.
fn win_a_round(save: &std::path::Path) -> World {
    let save = save.to_path_buf();
    script_saving(Some(save), chase_nearest_coin).run_headless(ROUND_SECONDS as u64 * HZ, first_round)
}

/// Starts on the title screen with progress in `save`, runs `ticks`, returns the world.
fn open_game(save: Option<PathBuf>, ticks: u64) -> World {
    script_saving(save, |_| {}).run_headless(ticks, on_title)
}

// ---- Rounds -----------------------------------------------------------------

// ANCHOR: win
#[test]
fn collecting_every_coin_wins_the_round() {
    let mut world = script(chase_nearest_coin).run_headless(ROUND_SECONDS as u64 * HZ, first_round);
    let round = world.resource::<Round>().clone();
    assert_eq!(screen(&world), Screen::Result(Outcome::Won));
    assert_eq!((round.collected, round.number), (COINS, 1));
    assert!(round.time_left > 0.0, "won with time to spare, got {}", round.time_left);
    assert_eq!(world.query::<&Coin>().iter().count(), 0);
}
// ANCHOR_END: win

#[test]
fn a_finished_round_stays_finished_until_the_player_moves_on() {
    let mut frozen = None;
    let mut world = script(move |ctx| {
        chase_nearest_coin(ctx);
        // Once won, keep "walking" and let the clock run: nothing may change.
        if matches!(*ctx.world.resource::<Screen>(), Screen::Result(_)) {
            let round = ctx.world.resource::<Round>().clone();
            let seen = (round.collected, round.time_left);
            assert_eq!(*frozen.get_or_insert(seen), seen, "state changed after winning");
        }
    })
    .run_headless(ROUND_SECONDS as u64 * HZ, first_round);
    assert_eq!(screen(&world), Screen::Result(Outcome::Won));
    assert_eq!(world.query::<&Player>().iter().count(), 1);
}

#[test]
fn running_out_of_time_loses_the_round() {
    let ticks = ROUND_SECONDS as u64 * HZ + 10; // a few spare ticks for rounding
    let world = script(|_| {}).run_headless(ticks, first_round);
    let round = world.resource::<Round>();
    assert_eq!(screen(&world), Screen::Result(Outcome::Lost));
    assert_eq!((round.collected, round.time_left), (0, 0.0));
}

/// Plays one round with `play`, asks for a new round on the first tick the
/// result screen shows, and stops on that tick.
fn restart_when_over(mut play: impl FnMut(&mut Ctx) + 'static) -> impl FnMut(&mut Ctx) + 'static {
    move |ctx| {
        if screen(&ctx.world) == Screen::Playing {
            play(ctx);
        } else {
            intent(ctx).confirm = true;
            ctx.quit(); // the tick still finishes, so `rules` handles it
        }
    }
}

fn assert_fresh_round(world: &mut World, number: u32) {
    let round = world.resource::<Round>().clone();
    assert_eq!(screen(world), Screen::Playing);
    assert_eq!((round.number, round.collected, round.new_best), (number, 0, false));
    assert_eq!(round.time_left, ROUND_SECONDS);
    assert_eq!(world.query::<&Coin>().iter().count(), COINS);
    let players: Vec<Vec2> = world
        .query::<(&Transform2D, &Player)>()
        .iter()
        .map(|(t, _)| t.position)
        .collect();
    assert_eq!(players, [ARENA / 2.0], "one player, back in the middle");
}

#[test]
fn playing_again_after_a_win_starts_a_fresh_round() {
    let mut world =
        script(restart_when_over(chase_nearest_coin)).run_headless(2 * ROUND_SECONDS as u64 * HZ, first_round);
    assert_fresh_round(&mut world, 2);
}

#[test]
fn playing_again_after_a_loss_starts_a_fresh_round() {
    let mut world = script(restart_when_over(|_| {})).run_headless(2 * ROUND_SECONDS as u64 * HZ, first_round);
    assert_fresh_round(&mut world, 2);
}

#[test]
fn restart_works_in_the_middle_of_a_round_too() {
    let taken_before_restart = Rc::new(Cell::new(0));
    let seen = taken_before_restart.clone();
    let mut world = script(move |ctx| {
        chase_nearest_coin(ctx);
        if ctx.time.sim_tick() == 2 * HZ {
            seen.set(ctx.world.resource::<Round>().collected);
            intent(ctx).restart = true;
            ctx.quit();
        }
    })
    .run_headless(10 * HZ, first_round);
    assert!(taken_before_restart.get() > 0, "the round was under way");
    assert_fresh_round(&mut world, 2);
}

#[test]
fn the_next_round_can_be_won_too() {
    // Win, play again, win again: the whole loop twice.
    let mut world = script(move |ctx| {
        if matches!(*ctx.world.resource::<Screen>(), Screen::Result(_)) && ctx.world.resource::<Round>().number == 1 {
            intent(ctx).confirm = true;
        }
        chase_nearest_coin(ctx);
    })
    .run_headless(2 * ROUND_SECONDS as u64 * HZ, first_round);
    let round = world.resource::<Round>().clone();
    assert_eq!(
        (screen(&world), round.number, round.collected),
        (Screen::Result(Outcome::Won), 2, COINS)
    );
    assert_eq!(world.query::<&Coin>().iter().count(), 0);
}

#[test]
fn rounds_are_reproducible_and_each_round_has_its_own_layout() {
    let layout_after = |ticks| coin_layout(&mut script(|_| {}).run_headless(ticks, first_round));
    assert_eq!(layout_after(0), layout_after(0), "same seed, same coins");
    assert_eq!(layout_after(0).len(), COINS);

    // Same inputs on both runs: identical games, tick for tick.
    let play =
        || script(restart_when_over(chase_nearest_coin)).run_headless(2 * ROUND_SECONDS as u64 * HZ, first_round);
    let (mut a, mut b) = (play(), play());
    assert_eq!(coin_layout(&mut a), coin_layout(&mut b));
    assert_eq!(a.resource::<Round>().time_left, b.resource::<Round>().time_left);

    // Round 2 is not round 1 again.
    assert_ne!(layout_after(0), coin_layout(&mut a));
}

#[test]
fn the_player_stays_in_the_arena_and_diagonals_are_not_faster() {
    let ticks = HZ; // one second
    let mut world = script(|ctx| intent(ctx).direction = vec2(1.0, 1.0)).run_headless(1, first_round);
    let moved = world
        .query::<(&Transform2D, &Player)>()
        .iter()
        .next()
        .unwrap()
        .0
        .position
        - ARENA / 2.0;
    let one_tick = SPEED / HZ as f32;
    assert!(
        (moved.length() - one_tick).abs() < 1e-3,
        "moved {moved}, expected length {one_tick}"
    );

    let mut world = script(|ctx| intent(ctx).direction = vec2(1.0, 1.0)).run_headless(ticks * 10, first_round);
    let at = world
        .query::<(&Transform2D, &Player)>()
        .iter()
        .next()
        .unwrap()
        .0
        .position;
    assert_eq!(at, ARENA, "pinned in the bottom-right corner");
}

// ---- Title, pause and result screens ----------------------------------------

#[test]
fn the_game_starts_on_an_empty_title_screen_that_ignores_movement() {
    let world = script(|ctx| intent(ctx).direction = vec2(1.0, 0.0)).run_headless(2 * HZ, on_title);
    assert_eq!(screen(&world), Screen::Title);
    assert_eq!(world.resource::<Round>().number, 0);
    assert!(world.is_empty(), "no player and no coins before the first round");
}

#[test]
fn confirm_on_the_title_screen_starts_round_one() {
    let mut world = script(|ctx| {
        if ctx.time.sim_tick() == 3 {
            intent(ctx).confirm = true;
            ctx.quit();
        }
    })
    .run_headless(10, on_title);
    assert_fresh_round(&mut world, 1);
}

#[test]
fn restart_and_pause_do_not_start_a_round_from_the_title_screen() {
    let world = script(|ctx| {
        let i = intent(ctx);
        i.restart = true;
        i.direction = vec2(1.0, 1.0);
    })
    .run_headless(5, on_title);
    assert_eq!(screen(&world), Screen::Title);
}

#[test]
fn pause_or_back_on_the_title_screen_quits_the_game() {
    for press in [|i: &mut Intent| i.pause = true, |i: &mut Intent| i.back = true] {
        let ticks = Rc::new(Cell::new(0));
        let counted = ticks.clone();
        script(move |ctx| {
            counted.set(counted.get() + 1);
            if ctx.time.sim_tick() == 2 {
                press(intent(ctx));
            }
        })
        .run_headless(100, on_title);
        assert_eq!(ticks.get(), 3, "the game stops on the tick after the request");
    }
}

#[test]
fn pausing_freezes_the_round_and_resuming_continues_it() {
    // Chase for one second, pause for five, resume.
    let at_pause = Rc::new(RefCell::new(None));
    let record = at_pause.clone();
    let mut world = script(move |ctx| {
        let tick = ctx.time.sim_tick();
        if tick == HZ {
            intent(ctx).pause = true;
        }
        if tick == 6 * HZ {
            intent(ctx).confirm = true; // resume
        }
        let snapshot = {
            let round = ctx.world.resource::<Round>();
            (round.collected, round.time_left, player_and_coins(ctx))
        };
        if screen(&ctx.world) == Screen::Paused {
            // Held movement and coins in reach must change nothing while paused.
            let first = record.borrow_mut().get_or_insert(snapshot.clone()).clone();
            assert_eq!(first, snapshot, "something moved at tick {tick} while paused");
        }
        chase_nearest_coin(ctx);
    })
    .run_headless(6 * HZ + 1, first_round);
    assert!(at_pause.borrow().is_some(), "the game was paused");
    // Resumed on the last tick: the clock runs again from where it stopped.
    assert_eq!(screen(&world), Screen::Playing);
    let (_, time_at_pause, _) = at_pause.borrow().clone().unwrap();
    let time_left = world.resource::<Round>().time_left;
    assert!(
        time_left <= time_at_pause && time_at_pause - time_left < 0.1,
        "{time_at_pause} -> {time_left}"
    );
    assert!(
        time_left > ROUND_SECONDS - 2.0,
        "five paused seconds were not counted: {time_left}"
    );
    assert_eq!(world.query::<&Player>().iter().count(), 1);
}

#[test]
fn pause_toggles_with_the_pause_button_and_confirm_resumes() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    script(move |ctx| {
        log.borrow_mut().push(screen(&ctx.world));
        match ctx.time.sim_tick() {
            1 | 5 => intent(ctx).pause = true, // pause ...
            3 => intent(ctx).confirm = true,   // ... resume with confirm
            7 => intent(ctx).pause = true,     // ... resume with pause
            8 => intent(ctx).confirm = true,   // confirm while playing does nothing
            _ => {}
        }
    })
    .run_headless(10, first_round);
    use Screen::*;
    // Each entry is the screen at the start of a tick, before that tick's rules ran,
    // so a request made on tick `t` shows on tick `t + 1`.
    assert_eq!(
        *seen.borrow(),
        [
            Playing, Playing, Paused, Paused, Playing, Playing, Paused, Paused, Playing, Playing
        ]
    );
}

#[test]
fn restart_from_the_pause_screen_starts_a_fresh_round_and_back_returns_to_the_title() {
    let mut world = script(|ctx| {
        match ctx.time.sim_tick() {
            HZ => intent(ctx).pause = true,
            n if n == HZ + 2 => {
                intent(ctx).restart = true;
                ctx.quit();
            }
            _ => {}
        }
        chase_nearest_coin(ctx);
    })
    .run_headless(5 * HZ, first_round);
    assert_fresh_round(&mut world, 2);

    let world = script(|ctx| match ctx.time.sim_tick() {
        5 => intent(ctx).pause = true,
        7 => {
            intent(ctx).back = true;
            ctx.quit();
        }
        _ => {}
    })
    .run_headless(20, first_round);
    assert_eq!(screen(&world), Screen::Title);
    assert!(world.is_empty(), "the field is cleared");
    assert_eq!(world.resource::<Round>().number, 1, "the round counter goes on");
}

#[test]
fn back_does_nothing_while_playing() {
    let world = script(|ctx| intent(ctx).back = true).run_headless(HZ, first_round);
    assert_eq!(screen(&world), Screen::Playing);
}

#[test]
fn result_screen_offers_play_again_and_back_to_the_title() {
    // Title -> play -> lose -> play again (round 2) -> lose -> back to title.
    let world = script(|ctx| {
        let over = matches!(*ctx.world.resource::<Screen>(), Screen::Result(_));
        let number = ctx.world.resource::<Round>().number;
        if over {
            if number == 1 {
                intent(ctx).restart = true; // the restart button works here too
            } else {
                intent(ctx).back = true;
                ctx.quit();
            }
        } else if screen(&ctx.world) == Screen::Title {
            intent(ctx).confirm = true;
        }
    })
    .run_headless(5 * ROUND_SECONDS as u64 * HZ, on_title);
    assert_eq!(screen(&world), Screen::Title);
    let round = world.resource::<Round>();
    assert_eq!(round.number, 2, "title, round 1, round 2");
    assert!(world.is_empty());
}

#[test]
fn pause_on_the_result_screen_goes_back_to_the_title() {
    let world = script(|ctx| {
        if matches!(*ctx.world.resource::<Screen>(), Screen::Result(_)) {
            intent(ctx).pause = true;
            ctx.quit();
        }
    })
    .run_headless(2 * ROUND_SECONDS as u64 * HZ, first_round);
    assert_eq!(screen(&world), Screen::Title);
}

#[test]
fn each_screen_shows_its_own_text() {
    let text = |world: &World| -> String {
        screen_lines(world)
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let title = open_game(None, 0);
    let text_title = text(&title);
    assert!(
        text_title.contains("COIN DASH") && text_title.contains("Enter: play"),
        "{text_title}"
    );
    assert!(text_title.contains("No rounds played yet"), "{text_title}");

    let playing = script(|_| {}).run_headless(1, first_round);
    assert!(
        screen_lines(&playing).is_empty(),
        "the HUD line is drawn by hud, nothing else"
    );

    let paused = script(|ctx| {
        if ctx.time.sim_tick() == 1 {
            intent(ctx).pause = true;
        }
    })
    .run_headless(3, first_round);
    assert!(text(&paused).contains("Paused"), "{}", text(&paused));

    let won = script(chase_nearest_coin).run_headless(ROUND_SECONDS as u64 * HZ, first_round);
    let text_won = text(&won);
    assert!(
        text_won.contains("You win!") && text_won.contains("Time "),
        "{text_won}"
    );
    assert!(text_won.contains("Enter: play again"), "{text_won}");

    let lost = script(|_| {}).run_headless(ROUND_SECONDS as u64 * HZ + 10, first_round);
    let text_lost = text(&lost);
    assert!(
        text_lost.contains("Time's up") && text_lost.contains(&format!("Coins 0/{COINS}")),
        "{text_lost}"
    );
}

// ---- Saved progress ---------------------------------------------------------

#[test]
fn progress_keeps_the_best_time_and_the_most_coins() {
    let mut p = Progress::default();
    // A loss with 3 coins: a high score but no time.
    assert!(p.record(Outcome::Lost, 3, ROUND_SECONDS));
    assert_eq!(
        (p.best_time, p.best_coins, p.rounds_played, p.rounds_won),
        (None, 3, 1, 0)
    );
    // A win in 14 s: best time, and best coins (8 > 3).
    assert!(p.record(Outcome::Won, COINS, 14.0));
    assert_eq!(
        (p.best_time, p.best_coins, p.rounds_played, p.rounds_won),
        (Some(14.0), COINS, 2, 1)
    );
    // A slower win, then a worse loss: no new best, counters still count.
    assert!(!p.record(Outcome::Won, COINS, 15.0));
    assert!(!p.record(Outcome::Lost, 1, ROUND_SECONDS));
    assert_eq!(
        (p.best_time, p.best_coins, p.rounds_played, p.rounds_won),
        (Some(14.0), COINS, 4, 2)
    );
    // A faster win beats the time.
    assert!(p.record(Outcome::Won, COINS, 9.5));
    assert_eq!(p.best_time, Some(9.5));
}

#[test]
fn a_finished_round_is_saved_and_the_next_run_starts_from_it() {
    let dir = temp_dir("roundtrip");
    let path = dir.join("save").join("progress.json"); // the folder doesn't exist yet
    let won = win_a_round(&path);
    let round = won.resource::<Round>().clone();
    let elapsed = ROUND_SECONDS - round.time_left;
    assert!(round.new_best, "the first win is a best");
    assert!(!won.resource::<SaveFile>().save_failed);

    // A new run (a new process, in effect) reads it back from the title screen.
    let again = open_game(Some(path.clone()), 0);
    let progress = again.resource::<Progress>().clone();
    assert_eq!(again.resource::<SaveFile>().load, LoadStatus::Loaded);
    assert_eq!(
        progress,
        Progress {
            version: SAVE_VERSION,
            best_time: Some(elapsed),
            best_coins: COINS,
            rounds_played: 1,
            rounds_won: 1
        }
    );
    let title: Vec<String> = screen_lines(&again).into_iter().map(|l| l.text).collect();
    assert!(
        title.iter().any(|t| t.contains(&format!("Best time {elapsed:.1} s"))),
        "{title:?}"
    );
    assert!(
        !path.with_extension("json.tmp").exists(),
        "no temporary file is left behind"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_round_adds_to_the_saved_progress_without_losing_a_better_best() {
    let dir = temp_dir("accumulate");
    let path = dir.join("progress.json");
    // A saved best that no scripted run can beat (0.5 s), from an earlier run.
    let earlier = Progress {
        version: SAVE_VERSION,
        best_time: Some(0.5),
        best_coins: COINS,
        rounds_played: 4,
        rounds_won: 3,
    };
    earlier.save(&mut storage_of(&path)).unwrap();
    let won = win_a_round(&path);
    assert!(!won.resource::<Round>().new_best, "slower than 0.5 s");
    let (saved, status) = Progress::load(&mut storage_of(&path));
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!(
        saved,
        Progress {
            rounds_played: 5,
            rounds_won: 4,
            ..earlier
        }
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_save_file_is_a_first_run_and_is_only_written_when_a_round_ends() {
    let dir = temp_dir("missing");
    let path = dir.join("progress.json");
    let world = open_game(Some(path.clone()), 5);
    assert_eq!(world.resource::<SaveFile>().load, LoadStatus::Fresh);
    assert_eq!(*world.resource::<Progress>(), Progress::default());
    assert!(
        !path.exists(),
        "opening the game and staying on the title writes nothing"
    );

    // Quitting a round halfway (back to the title) doesn't count either.
    let quit_midway = script_saving(Some(path.clone()), |ctx| match ctx.time.sim_tick() {
        0 => intent(ctx).confirm = true,
        30 => intent(ctx).pause = true,
        32 => intent(ctx).back = true,
        _ => {}
    })
    .run_headless(40, on_title);
    assert_eq!(screen(&quit_midway), Screen::Title);
    assert_eq!(quit_midway.resource::<Progress>().rounds_played, 0);
    assert!(!path.exists());

    win_a_round(&path);
    assert!(path.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_save_file_is_replaced_by_fresh_progress_and_kept_as_a_backup() {
    let valid_but_impossible = [
        r#"{"version":1,"best_time":-3.0}"#,
        r#"{"version":1,"best_time":0.0}"#,
        r#"{"version":1,"best_time":999.0}"#,
        r#"{"version":1,"best_coins":99}"#,
        r#"{"version":1,"rounds_played":1,"rounds_won":5}"#,
        r#"{"version":7,"best_coins":2}"#, // from some other version of the game
        r#"{"best_coins":2}"#,             // no version
    ];
    let broken: [&[u8]; 6] = [
        b"",
        b"not json at all",
        b"{\"version\":1,\"best_coins\":",
        b"[]",
        b"{\"version\":1,\"best_coins\":\"many\"}",
        &[0xff, 0xfe, 0x00, 0x9f], // not even text
    ];
    let cases: Vec<Vec<u8>> = broken
        .iter()
        .map(|b| b.to_vec())
        .chain(valid_but_impossible.iter().map(|s| s.as_bytes().to_vec()))
        .collect();
    for (n, bytes) in cases.iter().enumerate() {
        let dir = temp_dir(&format!("damaged-{n}"));
        let path = dir.join("progress.json");
        std::fs::write(&path, bytes).unwrap();

        let world = open_game(Some(path.clone()), 0);
        let label = String::from_utf8_lossy(bytes).to_string();
        assert_eq!(world.resource::<SaveFile>().load, LoadStatus::Recovered, "{label:?}");
        assert_eq!(*world.resource::<Progress>(), Progress::default(), "{label:?}");
        assert_eq!(screen(&world), Screen::Title, "the game still starts: {label:?}");
        assert_eq!(
            std::fs::read(dir.join("progress.json.bad")).unwrap(),
            *bytes,
            "backup of {label:?}"
        );
        let title: Vec<String> = screen_lines(&world).into_iter().map(|l| l.text).collect();
        assert!(
            title.iter().any(|t| t.contains("unreadable")),
            "the player is told: {title:?}"
        );

        // And the game is playable and saves again: the next win replaces the file.
        win_a_round(&path);
        let (progress, status) = Progress::load(&mut storage_of(&path));
        assert_eq!(status, LoadStatus::Loaded, "{label:?}");
        assert_eq!((progress.rounds_played, progress.rounds_won), (1, 1), "{label:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn an_unreadable_save_location_does_not_stop_the_game() {
    // The "file" is a folder: it can't be read as a save, and can't be written.
    let dir = temp_dir("folder-as-file");
    let path = dir.join("progress.json");
    std::fs::create_dir(&path).unwrap();
    let world = open_game(Some(path.clone()), 0);
    assert_eq!(world.resource::<SaveFile>().load, LoadStatus::Recovered);
    assert_eq!(*world.resource::<Progress>(), Progress::default());

    // Losing a round tries to save, fails, says so, and the game carries on.
    let lost = script_saving(Some(path), |_| {}).run_headless(ROUND_SECONDS as u64 * HZ + 10, first_round);
    assert_eq!(screen(&lost), Screen::Result(Outcome::Lost));
    assert!(lost.resource::<SaveFile>().save_failed);
    let lines: Vec<String> = screen_lines(&lost).into_iter().map(|l| l.text).collect();
    assert!(lines.iter().any(|t| t.contains("could not be saved")), "{lines:?}");
    assert_eq!(lost.resource::<Progress>().rounds_played, 1, "still counted in memory");
    let _ = std::fs::remove_dir_all(dir);

    // A save path below a regular file can't be created either.
    let dir = temp_dir("below-a-file");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let won = win_a_round(&blocker.join("progress.json"));
    assert!(won.resource::<SaveFile>().save_failed);
    assert_eq!(screen(&won), Screen::Result(Outcome::Won));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_game_without_a_save_path_just_does_not_keep_progress() {
    let won = script(chase_nearest_coin).run_headless(ROUND_SECONDS as u64 * HZ, first_round);
    assert_eq!(won.resource::<SaveFile>().load, LoadStatus::Fresh);
    assert!(!won.resource::<SaveFile>().save_failed);
    assert_eq!(won.resource::<Progress>().rounds_won, 1);
}

// ---- Input ------------------------------------------------------------------

/// A scripted full session from raw key presses only: the real `read_input`
/// runs on every tick (as a sim system, since headless runs have no frames).
fn keyed(scenario: InputScenario, extra: impl FnMut(&mut Ctx) + 'static) -> Game {
    Game::new("coin dash keys", 800, 600)
        .seed(SEED)
        .input_scenario(scenario)
        .sim_system(read_input)
        .sim_system(extra)
        .sim_system(rules)
}

fn tap(scenario: InputScenario, tick: u64, key: Key) -> InputScenario {
    scenario
        .at(tick, InputEvent::Key(key, true))
        .at(tick + 1, InputEvent::Key(key, false))
}

#[test]
fn a_whole_session_can_be_played_from_key_presses() {
    let dir = temp_dir("session");
    let path = dir.join("progress.json");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();

    // Enter starts; Esc pauses; P resumes; the player then chases to a win;
    // Space plays again; R restarts mid-round; Esc, Q returns to the title.
    let mut scenario = InputScenario::new();
    for (tick, key) in [
        (5, Key::Enter),
        (30, Key::Escape),
        (60, Key::P),
        (1500, Key::Space),
        (1600, Key::R),
        (1700, Key::Escape),
        (1710, Key::Q),
    ] {
        scenario = tap(scenario, tick, key);
    }
    let world = keyed(scenario, move |ctx| {
        // The "player's hands" for the chase: movement only while a round runs.
        if screen(&ctx.world) == Screen::Playing {
            let (player, coins) = player_and_coins(ctx);
            let nearest = coins
                .into_iter()
                .min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)));
            // Same as holding the keys for the direction: written where read_input leaves it.
            intent(ctx).direction = nearest.map_or(Vec2::ZERO, |c| (c - player).normalize_or_zero());
        }
        log.borrow_mut().push((ctx.time.sim_tick(), screen(&ctx.world)));
    })
    .run_headless(1800, {
        let path = path.clone();
        move |ctx| setup_with(ctx, Some(path))
    });

    let at = |tick: u64| seen.borrow().iter().find(|(t, _)| *t == tick).unwrap().1;
    assert_eq!(at(4), Screen::Title);
    assert_eq!(at(10), Screen::Playing, "Enter started round 1");
    assert_eq!(at(40), Screen::Paused, "Escape paused");
    assert_eq!(at(70), Screen::Playing, "P resumed");
    assert_eq!(at(1400), Screen::Result(Outcome::Won), "round 1 won by chasing");
    assert_eq!(at(1510), Screen::Playing, "Space played again");
    assert_eq!(at(1650), Screen::Playing, "R restarted round 2 as round 3");
    assert_eq!(at(1705), Screen::Paused);
    assert_eq!(screen(&world), Screen::Title, "Q went back to the title");
    assert_eq!(world.resource::<Round>().number, 3);

    // The win was saved; the abandoned rounds were not counted.
    let (saved, status) = Progress::load(&mut storage_of(&path));
    assert_eq!(status, LoadStatus::Loaded);
    assert_eq!((saved.rounds_played, saved.rounds_won, saved.best_coins), (1, 1, COINS));
    assert_eq!(*world.resource::<Progress>(), saved);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn keys_and_gamepad_map_to_the_same_intent() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        setup_playing(ctx);
        // Keyboard: A is left, S is down.
        ctx.input.feed(InputEvent::Key(Key::A, true));
        ctx.input.feed(InputEvent::Key(Key::S, true));
        read_input(ctx);
        let d = ctx.world.resource::<Intent>().direction;
        assert!(d.x < 0.0 && d.y > 0.0, "left and down, got {d}");
        ctx.input.feed(InputEvent::Key(Key::A, false));
        ctx.input.feed(InputEvent::Key(Key::S, false));

        // Arrow keys do the same as WASD.
        ctx.input.feed(InputEvent::Key(Key::Right, true));
        read_input(ctx);
        assert!(ctx.world.resource::<Intent>().direction.x > 0.0);
        ctx.input.feed(InputEvent::Key(Key::Right, false));

        // The left stick, past its dead zone, pushes up.
        let pad = Gamepad::from_slot(0).unwrap();
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickY, -1.0));
        read_input(ctx);
        assert!(ctx.world.resource::<Intent>().direction.y < 0.0, "stick up");
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickY, 0.0));

        // Nothing held: standing still.
        read_input(ctx);
        assert_eq!(ctx.world.resource::<Intent>().direction, Vec2::ZERO);
    });
}

#[test]
fn every_button_binding_sets_its_own_request_which_the_sim_takes_once() {
    let pad = Gamepad::from_slot(0).unwrap();
    let key = |k| (InputEvent::Key(k, true), InputEvent::Key(k, false));
    let button = |b| {
        (
            InputEvent::GamepadButton(pad, b, true),
            InputEvent::GamepadButton(pad, b, false),
        )
    };
    type Flag = fn(&Intent) -> bool;
    type Press = (InputEvent, InputEvent); // press, release
    let table: [(&str, Flag, Vec<Press>); 4] = [
        (
            "confirm",
            |i| i.confirm,
            vec![key(Key::Enter), key(Key::Space), button(GamepadButton::South)],
        ),
        ("restart", |i| i.restart, vec![key(Key::R), button(GamepadButton::West)]),
        (
            "pause",
            |i| i.pause,
            vec![key(Key::Escape), key(Key::P), button(GamepadButton::Start)],
        ),
        ("back", |i| i.back, vec![key(Key::Q), button(GamepadButton::East)]),
    ];
    for (name, flag, presses) in table {
        for (press, release) in presses {
            // A fresh game per press: the engine keeps "pressed" until the frame ends.
            Game::new("t", 800, 600).run_headless(0, |ctx| {
                setup_with(ctx, None);
                assert!(!flag(ctx.world.resource::<Intent>()), "{name}: not pressed yet");
                ctx.input.feed(press);
                read_input(ctx);
                ctx.input.feed(release);
                // A frame without a sim tick must not lose it.
                assert!(flag(ctx.world.resource::<Intent>()), "{press:?} sets {name}");
                // Only that one button: the others are untouched.
                let i = *ctx.world.resource::<Intent>();
                let set = [i.confirm, i.restart, i.pause, i.back];
                assert_eq!(set.iter().filter(|s| **s).count(), 1, "{press:?}: {set:?}");
                rules(ctx);
                assert!(!flag(ctx.world.resource::<Intent>()), "{name}: taken exactly once");
            });
        }
    }
}

#[test]
fn every_control_name_is_a_binding() {
    // `Act` resolves names once; a name `bindings` doesn't define would
    // read false forever, so the two lists must agree.
    let map = bindings();
    for name in Act::NAMES {
        assert!(map.resolve(name).is_ok(), "`{name}` is not bound");
    }
}

// ---- Settings: UI scale, volume and rebindable controls ---------------------

use game::settings::{self, Control, Controls, Refused, SCALES, Settings, SettingsFile};

/// All sixteen gamepad buttons: the engine can't list them, and the settings
/// screen needs no list (it asks what was pressed), but the rules around them
/// are checked for every one.
const PAD_BUTTONS: [GamepadButton; 16] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::North,
    GamepadButton::West,
    GamepadButton::LeftBumper,
    GamepadButton::RightBumper,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::Select,
    GamepadButton::Start,
    GamepadButton::LeftStick,
    GamepadButton::RightStick,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

// ANCHOR: ui_drive
/// What the engine does with the retained UI at the start of every frame (the
/// keyed labels' text in the current language, layout, focus, events from
/// this input), then the game's own frame system.
fn ui_frame(ctx: &mut Ctx, input: UiInput) {
    ctx.ui.localize(strings(&ctx.world));
    ctx.ui.frame(ARENA, &input);
    settings::settings_ui(ctx);
}

fn idle(ctx: &mut Ctx) {
    ui_frame(ctx, UiInput::default());
}

/// Presses Tab (or Shift-Tab) once.
fn tab(ctx: &mut Ctx, backwards: bool) {
    ui_frame(
        ctx,
        UiInput {
            next: !backwards,
            previous: backwards,
            shift: backwards,
            ..Default::default()
        },
    );
}

fn press_confirm(ctx: &mut Ctx) {
    ui_frame(
        ctx,
        UiInput {
            confirm: true,
            ..Default::default()
        },
    );
}

/// The name of the control that has the focus, as a screen reader would say it.
fn focused(ctx: &Ctx) -> Option<String> {
    let tree = ctx.ui.accessibility_tree();
    tree.nodes().iter().find(|n| n.focused).map(|n| n.name.clone())
}

// ANCHOR_END: ui_drive

/// Whether a control's accessible name is `name`, or `name` followed by its
/// current value (`"Confirm key: Enter"` is named `"Confirm key"`).
fn named(accessible_name: &str, name: &str) -> bool {
    accessible_name == name || accessible_name.starts_with(&format!("{name}:"))
}

/// Tabs until the control called `name` has the focus.
fn tab_to(ctx: &mut Ctx, name: &str) {
    for _ in 0..60 {
        if focused(ctx).is_some_and(|f| named(&f, name)) {
            return;
        }
        tab(ctx, false);
    }
    panic!(
        "Tab never reached \"{name}\"; the screen is:\n{}",
        ctx.ui.accessibility_tree()
    );
}

/// The bounds of the control called `name`, after the last frame.
fn bounds(ctx: &Ctx, name: &str) -> Rect {
    let tree = ctx.ui.accessibility_tree();
    tree.nodes()
        .iter()
        .find(|n| named(&n.name, name))
        .unwrap_or_else(|| panic!("no \"{name}\" on screen:\n{tree}"))
        .rect
}

/// Starts with settings kept beside `save`, and opens the settings screen the way a player does.
fn open_settings(ctx: &mut Ctx, save: Option<PathBuf>) {
    setup_with(ctx, save);
    intent(ctx).settings = true;
    rules(ctx);
    assert_eq!(screen(&ctx.world), Screen::Settings);
    idle(ctx); // builds and shows the screen
    idle(ctx); // lays it out
}

fn settings_of(ctx: &Ctx) -> Settings {
    ctx.world.resource::<Settings>().clone()
}

#[test]
fn the_settings_screen_opens_from_the_title_and_leaves_with_its_back_button() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        assert!(
            ctx.ui.wants_keyboard(),
            "the retained UI owns the keyboard while it is open"
        );
        tab_to(ctx, "Back");
        press_confirm(ctx);
        assert!(intent(ctx).close_settings, "the Back button asks to close");
        rules(ctx);
        assert_eq!(screen(&ctx.world), Screen::Title);
        idle(ctx); // the game takes the screen off the UI's stack ...
        assert!(ctx.ui.top_screen().is_none());
        idle(ctx); // ... and the next frame's input is the game's again
        assert!(!ctx.ui.wants_keyboard(), "the screen was taken off the UI's stack");
        assert!(ctx.ui.top_screen().is_none());
    });
}

#[test]
fn escape_and_the_east_button_close_the_settings_screen_too() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        // The engine pops a screen on cancel; the game follows.
        ui_frame(
            ctx,
            UiInput {
                cancel: true,
                ..Default::default()
            },
        );
        assert!(intent(ctx).close_settings);
        rules(ctx);
        assert_eq!(screen(&ctx.world), Screen::Title);
        idle(ctx);
        assert!(ctx.ui.top_screen().is_none());

        // And it can be opened again.
        intent(ctx).settings = true;
        rules(ctx);
        idle(ctx);
        assert!(ctx.ui.wants_keyboard());
    });
}

#[test]
fn tab_shift_tab_and_arrows_reach_every_control_in_a_sensible_order() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        let mut order = Vec::new();
        for _ in 0..40 {
            tab(ctx, false);
            let now = focused(ctx).expect("the screen has focusable controls");
            if order.contains(&now) {
                break; // wrapped around
            }
            order.push(now);
        }
        // Scale, volume, language, then each control's key and gamepad button, then the footer.
        assert_eq!(
            order[..5],
            [
                "Smaller UI",
                "Larger UI",
                "Volume",
                "Language: English",
                "Move up key: W"
            ]
        );
        assert!(
            order.contains(&"Confirm gamepad button: South".to_string()),
            "{order:?}"
        );
        assert!(order.contains(&"Settings key: O".to_string()), "{order:?}");
        assert_eq!(order[order.len() - 2..], ["Reset to defaults", "Back"]);
        // 2 scale buttons, the slider, the language, 9 keys, 5 gamepad buttons, 2 footer buttons.
        assert_eq!(order.len(), 2 + 1 + 1 + 9 + 5 + 2, "{order:?}");

        // Shift-Tab goes back, and the arrows move between neighbours.
        tab_to(ctx, "Larger UI");
        tab(ctx, true);
        assert_eq!(focused(ctx).as_deref(), Some("Smaller UI"));
        let before = focused(ctx);
        ui_frame(
            ctx,
            UiInput {
                direction: vec2(1.0, 0.0),
                ..Default::default()
            },
        );
        assert_eq!(focused(ctx).as_deref(), Some("Larger UI"), "right from {before:?}");
        ui_frame(
            ctx,
            UiInput {
                direction: vec2(0.0, 1.0),
                ..Default::default()
            },
        );
        assert_ne!(focused(ctx).as_deref(), Some("Larger UI"), "down leaves the scale row");
    });
}

#[test]
fn the_ui_scale_steps_through_the_offered_sizes_and_scales_the_screen() {
    assert_eq!(SCALES, [0.75, 1.0, 1.25, 1.5]);
    assert_eq!(
        (
            settings::step_scale(1.0, 1),
            settings::step_scale(1.5, 1),
            settings::step_scale(0.75, -1)
        ),
        (1.25, 1.5, 0.75),
        "stops at the ends"
    );
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        let back = bounds(ctx, "Back").size();
        assert_eq!(back, vec2(120.0, 36.0), "100%: the sizes the layout asks for");
        tab_to(ctx, "Larger UI");
        press_confirm(ctx);
        assert_eq!(settings_of(ctx).ui_scale, 1.25);
        idle(ctx);
        assert_eq!(bounds(ctx, "Back").size(), back * 1.25, "geometry scales with it");
        let hit = bounds(ctx, "Back").center();
        assert_eq!(ctx.ui.hit_test(hit).map(|id| ctx.ui.text(id)), Some(Some("Back")));
        // The screen still fits the window at the largest size.
        tab_to(ctx, "Larger UI");
        press_confirm(ctx);
        press_confirm(ctx); // already the largest: stays
        assert_eq!(settings_of(ctx).ui_scale, 1.5);
        idle(ctx);
        for node in ctx.ui.accessibility_tree().nodes() {
            if node.rect.size().x > 0.0 && node.depth <= 2 {
                assert!(
                    node.rect.max.x <= ARENA.x + 0.5 && node.rect.min.x >= -0.5,
                    "\"{}\" is outside the window at 150%: {:?}",
                    node.name,
                    node.rect
                );
            }
        }
        tab_to(ctx, "Smaller UI");
        for _ in 0..4 {
            press_confirm(ctx);
        }
        assert_eq!(settings_of(ctx).ui_scale, 0.75);
    });
}

#[test]
fn the_volume_slider_changes_the_saved_volume() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        assert_eq!(settings_of(ctx).volume, Settings::default().volume);
        tab_to(ctx, "Volume");
        // The slider steps by 0.05 on the left and right arrows.
        for _ in 0..20 {
            ui_frame(
                ctx,
                UiInput {
                    direction: vec2(-1.0, 0.0),
                    ..Default::default()
                },
            );
        }
        assert_eq!(settings_of(ctx).volume, 0.0, "all the way down");
        ui_frame(
            ctx,
            UiInput {
                direction: vec2(1.0, 0.0),
                ..Default::default()
            },
        );
        let volume = settings_of(ctx).volume;
        assert!((volume - 0.05).abs() < 1e-4, "{volume}");
    });
}

/// Rebinds `control`'s key through the screen: focus its button, press it, then press `key`.
fn rebind_key_through_the_screen(ctx: &mut Ctx, control: Control, key: Key) {
    let name = format!("{} key", control.label(strings(&ctx.world)));
    tab_to(ctx, &name);
    press_confirm(ctx); // opens the prompt; the Enter that did it must not answer it
    idle(ctx);
    ctx.input.feed(InputEvent::Key(key, true));
    idle(ctx);
    ctx.input.feed(InputEvent::Key(key, false));
}

// ANCHOR: rebind_test
#[test]
fn rebinding_a_key_changes_what_the_game_reads() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        tab_to(ctx, "Restart round key");
        press_confirm(ctx);
        assert!(
            ctx.ui
                .top_screen()
                .is_some_and(|top| ctx.ui.accessibility_tree().get(top).is_some_and(|n| n.modal)),
            "the prompt is a modal screen"
        );
        assert_eq!(
            settings_of(ctx).controls.key(Control::Restart),
            Key::R,
            "the opening Enter did not answer it"
        );

        // The player presses K.
        ctx.input.feed(InputEvent::Key(Key::K, true));
        idle(ctx);
        ctx.input.feed(InputEvent::Key(Key::K, false));
        assert_eq!(settings_of(ctx).controls.key(Control::Restart), Key::K);
        assert!(
            ctx.ui
                .top_screen()
                .is_some_and(|top| !ctx.ui.accessibility_tree().get(top).is_some_and(|n| n.modal)),
            "the prompt closed"
        );
        assert_eq!(
            focused(ctx).as_deref(),
            Some("Restart round key: K"),
            "the button shows and names the new key"
        );

        // The installed bindings follow: K is "restart" now, R is nothing.
        ctx.input.feed(InputEvent::Key(Key::R, true));
        read_input(ctx);
        assert!(!intent(ctx).restart, "the old key does nothing");
        ctx.input.feed(InputEvent::Key(Key::R, false));
        ctx.input.feed(InputEvent::Key(Key::K, true));
        read_input(ctx);
        assert!(intent(ctx).restart, "the new key restarts");
        ctx.input.feed(InputEvent::Key(Key::K, false));
        assert!(
            screen_lines(&ctx.world).is_empty(),
            "settings screen text is the retained UI's"
        );
    });
}
// ANCHOR_END: rebind_test

#[test]
fn the_hints_on_the_other_screens_show_the_rebound_keys() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        rebind_key_through_the_screen(ctx, Control::Confirm, Key::K);
        rebind_key_through_the_screen(ctx, Control::Back, Key::B);
        intent(ctx).close_settings = true;
        rules(ctx);
        let title: Vec<String> = screen_lines(&ctx.world).into_iter().map(|l| l.text).collect();
        assert!(title.iter().any(|t| t.starts_with("K: play")), "{title:?}");
        ctx.world.insert_resource(Screen::Paused);
        let paused: Vec<String> = screen_lines(&ctx.world).into_iter().map(|l| l.text).collect();
        assert!(paused.iter().any(|t| t.contains("B: title")), "{paused:?}");
    });
}

#[test]
fn a_refused_key_keeps_the_prompt_open_and_says_why_and_escape_cancels() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        tab_to(ctx, "Confirm key");
        press_confirm(ctx);
        idle(ctx);
        let prompt_open = |ctx: &Ctx| ctx.ui.accessibility_tree().nodes().iter().any(|n| n.modal);
        let prompt_text = |ctx: &Ctx| {
            ctx.ui
                .accessibility_tree()
                .nodes()
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        // Q belongs to "Back to title", P and Space always work for something else.
        for (key, said) in [
            (Key::Q, "already used for \"Back to title\""),
            (Key::P, "always works for something else"),
            (Key::Space, "always works for something else"),
            (Key::Left, "always works for something else"),
        ] {
            ctx.input.feed(InputEvent::Key(key, true));
            idle(ctx);
            ctx.input.feed(InputEvent::Key(key, false));
            idle(ctx);
            assert_eq!(
                settings_of(ctx).controls,
                Controls::default(),
                "{key:?} changed something"
            );
            assert!(prompt_open(ctx), "the prompt stays for another try after {key:?}");
            assert!(prompt_text(ctx).contains(said), "{key:?}: {}", prompt_text(ctx));
        }
        // Escape cancels the prompt (the engine pops it) and changes nothing.
        ui_frame(
            ctx,
            UiInput {
                cancel: true,
                ..Default::default()
            },
        );
        assert!(!prompt_open(ctx));
        ctx.input.feed(InputEvent::Key(Key::Escape, true));
        idle(ctx);
        ctx.input.feed(InputEvent::Key(Key::Escape, false));
        assert_eq!(settings_of(ctx).controls, Controls::default());
        assert_eq!(screen(&ctx.world), Screen::Settings, "Escape closed only the prompt");
        assert!(!intent(ctx).close_settings);
    });
}

#[test]
fn reset_to_defaults_restores_controls_scale_and_volume() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        rebind_key_through_the_screen(ctx, Control::Pause, Key::X);
        tab_to(ctx, "Larger UI");
        press_confirm(ctx);
        assert_ne!(settings_of(ctx), Settings::default());
        tab_to(ctx, "Reset to defaults");
        press_confirm(ctx);
        assert_eq!(settings_of(ctx), Settings::default());
        // The bindings really are the defaults again: Escape pauses.
        ctx.input.feed(InputEvent::Key(Key::Escape, true));
        read_input(ctx);
        assert!(intent(ctx).pause);
        ctx.input.feed(InputEvent::Key(Key::Escape, false));
    });
}

#[test]
fn chosen_settings_are_saved_when_leaving_and_come_back_next_run() {
    let dir = temp_dir("settings-roundtrip");
    let path = dir.join("progress.json");
    let seen = Rc::new(RefCell::new(None));
    let kept = seen.clone();
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, Some(path.clone()));
        rebind_key_through_the_screen(ctx, Control::Confirm, Key::K);
        tab_to(ctx, "Larger UI");
        press_confirm(ctx);
        press_confirm(ctx);
        tab_to(ctx, "Back");
        press_confirm(ctx);
        rules(ctx);
        assert_eq!(screen(&ctx.world), Screen::Title);
        assert!(!ctx.world.resource::<SettingsFile>().save_failed);
        *kept.borrow_mut() = Some(settings_of(ctx));
    });
    let chosen = seen.borrow().clone().unwrap();
    assert_eq!((chosen.ui_scale, chosen.controls.key(Control::Confirm)), (1.5, Key::K));
    assert!(dir.join("settings.json").exists());
    assert!(!dir.join("progress.json").exists(), "no progress was made");

    // A new run reads them back, and the bindings are installed.
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        setup_with(ctx, Some(path.clone()));
        assert_eq!(ctx.world.resource::<SettingsFile>().load, LoadStatus::Loaded);
        assert_eq!(settings_of(ctx), chosen);
        ctx.input.feed(InputEvent::Key(Key::K, true));
        read_input(ctx);
        assert!(intent(ctx).confirm, "K confirms after the restart");
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_settings_file_is_replaced_by_the_defaults_and_kept_as_a_backup() {
    let key = |s: &str| format!(r#"{{"version":1,"ui_scale":1.0,"volume":0.5,"controls":{s}}}"#);
    let mut duplicate = Controls::default();
    duplicate.keys[Control::Restart as usize] = Key::Enter;
    let mut fixed = Controls::default();
    fixed.keys[Control::Back as usize] = Key::Space;
    let mut pad_on_move = Controls::default();
    pad_on_move.pads[Control::MoveUp as usize] = Some(GamepadButton::North);
    let mut pad_on_dpad = Controls::default();
    pad_on_dpad.pads[Control::Restart as usize] = Some(GamepadButton::DPadRight);
    let mut east_elsewhere = Controls::default();
    east_elsewhere.pads[Control::Confirm as usize] = Some(GamepadButton::East);
    east_elsewhere.pads[Control::Back as usize] = Some(GamepadButton::South);
    let cases: Vec<String> = vec![
        String::new(),
        "not json".into(),
        r#"{"version":1,"ui_scale":"big"}"#.into(),
        r#"{"version":1,"ui_scale":3.0}"#.into(), // not one of the offered scales
        r#"{"version":1,"ui_scale":1.1}"#.into(),
        r#"{"version":1,"volume":2.0}"#.into(),
        r#"{"version":1,"volume":-0.1}"#.into(),
        r#"{"version":9}"#.into(),
        key(r#"{"keys":["Nope"]}"#),
        key(&serde_json::to_string(&duplicate).unwrap()),
        key(&serde_json::to_string(&fixed).unwrap()),
        key(&serde_json::to_string(&pad_on_move).unwrap()),
        key(&serde_json::to_string(&pad_on_dpad).unwrap()),
        key(&serde_json::to_string(&east_elsewhere).unwrap()),
    ];
    for (n, text) in cases.iter().enumerate() {
        let dir = temp_dir(&format!("settings-damaged-{n}"));
        let path = dir.join("progress.json");
        std::fs::write(dir.join("settings.json"), text).unwrap();
        let world = open_game(Some(path.clone()), 0);
        assert_eq!(world.resource::<SettingsFile>().load, LoadStatus::Recovered, "{text:?}");
        assert_eq!(*world.resource::<Settings>(), Settings::default(), "{text:?}");
        assert_eq!(
            std::fs::read(dir.join("settings.json.bad")).unwrap(),
            text.as_bytes(),
            "{text:?}"
        );
        assert_eq!(screen(&world), Screen::Title, "the game still starts: {text:?}");
        let title: Vec<String> = screen_lines(&world).into_iter().map(|l| l.text).collect();
        assert!(
            title.iter().any(|t| t.contains("settings were unreadable")),
            "{title:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_missing_settings_file_means_the_defaults_and_is_not_an_error() {
    let dir = temp_dir("settings-missing");
    let world = open_game(Some(dir.join("progress.json")), 0);
    assert_eq!(world.resource::<SettingsFile>().load, LoadStatus::Fresh);
    assert_eq!(*world.resource::<Settings>(), Settings::default());
    assert!(
        !dir.join("settings.json").exists(),
        "nothing is written until the settings are left"
    );
    let title: Vec<String> = screen_lines(&world).into_iter().map(|l| l.text).collect();
    assert!(!title.iter().any(|t| t.contains("unreadable")), "{title:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_settings_file_with_only_some_fields_keeps_the_defaults_for_the_rest() {
    // A file from before a field existed (or one written by hand) still loads.
    let dir = temp_dir("settings-partial");
    std::fs::write(dir.join("settings.json"), r#"{"version":1,"ui_scale":1.25}"#).unwrap();
    let world = open_game(Some(dir.join("progress.json")), 0);
    assert_eq!(world.resource::<SettingsFile>().load, LoadStatus::Loaded);
    let settings = world.resource::<Settings>();
    assert_eq!(settings.ui_scale, 1.25);
    assert_eq!(
        (settings.volume, &settings.controls),
        (Settings::default().volume, &Controls::default())
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_that_cannot_be_written_do_not_stop_the_game() {
    let dir = temp_dir("settings-readonly");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let world = Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, Some(blocker.join("progress.json")));
        intent(ctx).close_settings = true;
        rules(ctx);
        assert!(ctx.world.resource::<SettingsFile>().save_failed);
        assert_eq!(screen(&ctx.world), Screen::Title);
    });
    drop(world);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn controls_refuse_reserved_fixed_and_used_keys_and_buttons() {
    let mut c = Controls::default();
    assert!(c.is_valid());
    assert_eq!(c.set_key(Control::Confirm, Key::Escape), Err(Refused::Reserved));
    assert_eq!(c.set_key(Control::Confirm, Key::Tab), Err(Refused::Reserved));
    // Escape is Pause's default but not a choice: the prompt can't hear it (Escape
    // closes it), so the settings screen says "Reset gives Pause its Escape back".
    assert_eq!(c.set_key(Control::Pause, Key::X), Ok(()));
    assert_eq!(c.set_key(Control::Pause, Key::Escape), Err(Refused::Reserved));
    c = Controls::default();
    assert_eq!(c.set_key(Control::Confirm, Key::P), Err(Refused::Fixed));
    assert_eq!(c.set_key(Control::MoveUp, Key::Left), Err(Refused::Fixed));
    assert_eq!(c.set_key(Control::Confirm, Key::Q), Err(Refused::InUse(Control::Back)));
    assert_eq!(c, Controls::default(), "a refusal changes nothing");
    // Taking the key it already has is fine (and a swap is two steps).
    assert_eq!(c.set_key(Control::Back, Key::Q), Ok(()));
    assert_eq!(c.set_key(Control::Back, Key::B), Ok(()));
    assert_eq!(c.set_key(Control::Confirm, Key::Q), Ok(()));
    assert_eq!((c.key(Control::Confirm), c.key(Control::Back)), (Key::Q, Key::B));
    assert!(c.is_valid());

    assert_eq!(c.set_pad(Control::MoveUp, GamepadButton::North), Err(Refused::NoButton));
    assert_eq!(c.set_pad(Control::Confirm, GamepadButton::East), Err(Refused::Reserved));
    for dpad in [
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ] {
        assert_eq!(
            c.set_pad(Control::Confirm, dpad),
            Err(Refused::Reserved),
            "{dpad:?} navigates the menus"
        );
    }
    assert_eq!(
        c.set_pad(Control::Confirm, GamepadButton::West),
        Err(Refused::InUse(Control::Restart))
    );
    assert_eq!(c.set_pad(Control::Confirm, GamepadButton::LeftBumper), Ok(()));
    assert_eq!(c.pad(Control::Confirm), Some(GamepadButton::LeftBumper));
    assert!(c.is_valid());
}

#[test]
fn the_gamepad_prompt_reserves_what_the_menus_use() {
    // The list is all sixteen buttons, each once.
    let mut seen = PAD_BUTTONS.to_vec();
    seen.sort_by_key(|b| format!("{b:?}"));
    seen.dedup();
    assert_eq!(seen.len(), 16, "{PAD_BUTTONS:?}");
    // The retained UI navigates with the D-pad and closes with East (see
    // `retained_input` and the guide's "Events, navigation and ownership"): the
    // same four directions are refused for every control, as the arrows are on the keyboard.
    let reserved: Vec<GamepadButton> = PAD_BUTTONS
        .into_iter()
        .filter(|&b| Controls::default().set_pad(Control::Settings, b) == Err(Refused::Reserved))
        .collect();
    assert_eq!(
        reserved,
        [
            GamepadButton::East,
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::DPadLeft,
            GamepadButton::DPadRight
        ]
    );
    assert!(reserved.iter().all(|&b| settings::is_reserved_pad(b)));
    // Everything else can be taken by a control that doesn't have it yet.
    for button in PAD_BUTTONS.into_iter().filter(|b| !reserved.contains(b)) {
        let mut c = Controls::default();
        let taken = Control::ALL.into_iter().find(|&k| c.pad(k) == Some(button));
        let result = c.set_pad(Control::Settings, button);
        match taken {
            Some(other) if other != Control::Settings => assert_eq!(result, Err(Refused::InUse(other))),
            _ => assert_eq!(result, Ok(()), "{button:?}"),
        }
    }
}

#[test]
fn the_gamepad_prompt_refuses_the_dpad_in_the_screen_and_takes_another_button() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        tab_to(ctx, "Restart round gamepad button");
        press_confirm(ctx);
        idle(ctx);
        let pad = Gamepad::from_slot(0).unwrap();
        let prompt = |ctx: &Ctx| {
            ctx.ui
                .accessibility_tree()
                .nodes()
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        for dpad in [GamepadButton::DPadRight, GamepadButton::DPadLeft] {
            ctx.input.feed(InputEvent::GamepadButton(pad, dpad, true));
            idle(ctx);
            ctx.input.feed(InputEvent::GamepadButton(pad, dpad, false));
            idle(ctx);
            assert_eq!(
                settings_of(ctx).controls,
                Controls::default(),
                "{dpad:?} changed something"
            );
            assert!(
                prompt(ctx).contains("reserved for the menus"),
                "{dpad:?}: {}",
                prompt(ctx)
            );
        }
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::LeftBumper, true));
        idle(ctx);
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::LeftBumper, false));
        assert_eq!(
            settings_of(ctx).controls.pad(Control::Restart),
            Some(GamepadButton::LeftBumper)
        );
        assert_eq!(
            focused(ctx).as_deref(),
            Some("Restart round gamepad button: LeftBumper")
        );
        // The installed bindings follow.
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::LeftBumper, true));
        read_input(ctx);
        assert!(intent(ctx).restart, "the left bumper restarts now");
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::LeftBumper, false));
    });
}

#[test]
fn the_settings_screen_says_which_keys_belong_to_the_menus() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        open_settings(ctx, None);
        let tree = ctx.ui.accessibility_tree();
        let hint = tree
            .nodes()
            .iter()
            .find(|n| n.name == strings(&ctx.world).get("settings.hint"))
            .expect("the hint is on the screen");
        assert!(
            hint.name.contains("Escape")
                && hint.name.contains("D-pad")
                && hint.name.contains("Reset gives Pause its Escape back")
        );
        // Rebinding Pause away from Escape, then Reset, is the way back.
        rebind_key_through_the_screen(ctx, Control::Pause, Key::X);
        assert_eq!(settings_of(ctx).controls.key(Control::Pause), Key::X);
        tab_to(ctx, "Reset to defaults");
        press_confirm(ctx);
        assert_eq!(settings_of(ctx).controls.key(Control::Pause), Key::Escape);
    });
}

#[test]
fn rebound_controls_become_bindings_that_keep_the_resolved_action_ids() {
    let mut controls = Controls::default();
    controls.set_key(Control::MoveLeft, Key::J).unwrap();
    controls.set_pad(Control::Pause, GamepadButton::Select).unwrap();
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        setup_playing(ctx);
        let act = *ctx.world.resource::<Act>();
        ctx.input.set_actions(settings::bindings_for(&controls));
        // The ids resolved before the rebind still work: same names, same slots.
        ctx.input.feed(InputEvent::Key(Key::J, true));
        assert_eq!(ctx.input.action_axis2(act.movement).x, -1.0, "J moves left now");
        ctx.input.feed(InputEvent::Key(Key::J, false));
        ctx.input.feed(InputEvent::Key(Key::A, true));
        assert_eq!(ctx.input.action_axis2(act.movement).x, 0.0, "A no longer does");
        ctx.input.feed(InputEvent::Key(Key::A, false));
        ctx.input.feed(InputEvent::Key(Key::Left, true));
        assert_eq!(ctx.input.action_axis2(act.movement).x, -1.0, "the arrows are fixed");
        ctx.input.feed(InputEvent::Key(Key::Left, false));
        let pad = Gamepad::from_slot(0).unwrap();
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::Select, true));
        assert!(ctx.input.action_pressed(act.pause), "Select pauses now");
        ctx.input
            .feed(InputEvent::GamepadButton(pad, GamepadButton::Select, false));
    });
    // Saved controls survive a round trip through JSON.
    let json = serde_json::to_string(&controls).unwrap();
    assert_eq!(serde_json::from_str::<Controls>(&json).unwrap(), controls);
}

#[test]
fn controls_read_back_from_their_bindings_and_from_a_loaded_actions_file() {
    let mut rebound = Controls::default();
    rebound.set_key(Control::MoveUp, Key::I).unwrap();
    rebound.set_key(Control::Pause, Key::X).unwrap();
    rebound.set_pad(Control::Restart, GamepadButton::Select).unwrap();
    for controls in [Controls::default(), rebound] {
        let map = settings::bindings_for(&controls);
        assert_eq!(Controls::from_bindings(&map), controls);
        let loaded = ActionMap::from_json(&map.to_json().unwrap()).unwrap();
        assert_eq!(Controls::from_bindings(&loaded), controls);
    }
}

#[test]
fn the_settings_screen_shows_the_installed_bindings_not_a_copy() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        setup_with(ctx, None);
        // Bindings installed behind the settings' back, as a user-edited
        // actions file would be: the screen reads them from the map.
        let mut confirm = ctx.input.actions().get("confirm").unwrap().clone();
        let Action::Button(sources) = &mut confirm else {
            unreachable!()
        };
        sources[0] = ButtonBinding::Key(Key::G);
        ctx.input.set_action("confirm", confirm);
        assert_eq!(settings_of(ctx).controls.key(Control::Confirm), Key::Enter);
        intent(ctx).settings = true;
        rules(ctx);
        idle(ctx);
        idle(ctx);
        let tree = ctx.ui.accessibility_tree();
        assert!(
            tree.nodes().iter().any(|n| n.name == "Confirm key: G"),
            "the screen shows the bound key:\n{tree}"
        );
    });
}

#[test]
fn the_default_controls_are_the_default_bindings() {
    // `bindings()` is what a game without a settings file runs on.
    let from_controls = settings::bindings_for(&Controls::default());
    assert_eq!(bindings().to_json().unwrap(), from_controls.to_json().unwrap());
}

#[test]
fn every_screen_fits_the_window_at_every_ui_scale() {
    Game::new("t", 800, 600).run_headless(0, |ctx| {
        setup_with(ctx, None);
        // As many lines as a screen ever has: damaged saves and a failed write.
        ctx.world.resource_mut::<SaveFile>().load = LoadStatus::Recovered;
        ctx.world.resource_mut::<SaveFile>().save_failed = true;
        ctx.world.resource_mut::<SettingsFile>().load = LoadStatus::Recovered;
        ctx.world.resource_mut::<Round>().new_best = true;
        ctx.world.resource_mut::<Progress>().rounds_played = 12;
        ctx.world.resource_mut::<Progress>().rounds_won = 12;
        ctx.world.resource_mut::<Progress>().best_time = Some(12.3);
        ctx.world.resource_mut::<Progress>().best_coins = COINS;
        for scale in SCALES {
            ctx.world.resource_mut::<Settings>().ui_scale = scale;
            for shown in [
                Screen::Title,
                Screen::Paused,
                Screen::Result(Outcome::Won),
                Screen::Result(Outcome::Lost),
            ] {
                ctx.world.insert_resource(shown);
                let lines = screen_lines(&ctx.world);
                let mut height = 0.0;
                for l in &lines {
                    let width = ctx.gfx.measure_text(&l.text, l.size * scale).x;
                    assert!(width <= ARENA.x, "{shown:?} at {scale}: \"{}\" is {width} wide", l.text);
                    height += l.size * scale * 1.5;
                }
                assert!(height <= ARENA.y, "{shown:?} at {scale}: {height} tall");
            }
        }
    });
}

// ---- The packaged smoke walk-through ----------------------------------------

use game::smoke::{MAX_TICKS, Smoke, setup_smoke, smoke_game};

fn walk_through(save: &std::path::Path) -> game::smoke::Report {
    let game = Game::new("Coin Dash", ARENA.x as u32, ARENA.y as u32).data_dir(folder_of(save));
    let mut world = smoke_game(game).run_headless(MAX_TICKS + 1, |ctx| {
        setup_smoke(ctx, None, "headless");
    });
    let smoke = world.remove_resource::<Smoke>().unwrap();
    assert!(
        smoke.finished(),
        "the walk-through did not finish: {:?}",
        smoke.report()
    );
    smoke.report().clone()
}

#[test]
fn the_smoke_walk_through_plays_the_whole_game_and_reports_it() {
    let dir = temp_dir("smoke");
    let path = dir.join("data").join("progress.json");
    let report = walk_through(&path);
    assert_eq!(
        report.screens,
        ["Title", "Settings", "Title", "Playing", "Result(Won)", "Title"]
    );
    assert_eq!((report.rounds_played, report.rounds_won), (1, 1));
    assert!(report.best_time.is_some());
    assert_eq!(report.asset, "coin_dash/coin.png");
    assert_eq!(report.asset_size, [32, 32], "the coin picture was found and decoded");
    assert_eq!(
        (report.progress_at_start.as_str(), report.settings_at_start.as_str()),
        ("Fresh", "Fresh")
    );
    assert!(path.exists(), "progress was saved");
    assert!(
        dir.join("data").join("settings.json").exists(),
        "leaving the settings saved them"
    );

    // A second run starts from what the first one saved.
    let again = walk_through(&path);
    assert_eq!(
        (again.progress_at_start.as_str(), again.settings_at_start.as_str()),
        ("Loaded", "Loaded")
    );
    assert_eq!((again.rounds_played, again.rounds_won), (2, 2));

    // And a damaged file is recovered from, then written again.
    std::fs::write(&path, b"{ broken").unwrap();
    let recovered = walk_through(&path);
    assert_eq!(recovered.progress_at_start, "Recovered");
    assert_eq!(recovered.rounds_played, 1);
    assert!(dir.join("data").join("progress.json.bad").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_smoke_walk_through_gives_up_instead_of_hanging() {
    // A game that never leaves the title screen (the walk-through's own rules
    // are replaced by nothing): it ends at MAX_TICKS and says it did not finish.
    let mut world = Game::new("coin dash stuck", 800, 600)
        .seed(SEED)
        .sim_system(game::smoke::autoplay)
        .run_headless(MAX_TICKS + 1, |ctx| {
            setup_with(ctx, None);
            setup_smoke(ctx, None, "headless");
            ctx.world.insert_resource(Screen::Paused); // nothing here moves it on
        });
    let smoke = world.remove_resource::<Smoke>().unwrap();
    assert!(!smoke.finished());
}

// ---- The package's content notices ------------------------------------------

#[test]
fn the_content_notice_names_every_file_the_game_ships() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let notice = std::fs::read_to_string(root.join("examples/coin_dash/CONTENT-NOTICES.txt")).unwrap();
    let mut shipped = Vec::new();
    for entry in std::fs::read_dir(root.join("assets/coin_dash")).unwrap() {
        shipped.push(entry.unwrap().file_name().to_string_lossy().to_string());
    }
    assert!(shipped.contains(&"coin.png".to_string()), "{shipped:?}");
    for file in shipped {
        assert!(
            notice.contains(&format!("coin_dash/{file}")),
            "assets/coin_dash/{file} is shipped but not named in CONTENT-NOTICES.txt"
        );
    }
    assert!(notice.contains(game::COIN_ASSET));
}

#[test]
fn the_content_notice_uses_the_license_files_copyright_holder() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let license = std::fs::read_to_string(root.join("LICENSE-MIT")).unwrap();
    let holder = license
        .lines()
        .find_map(|l| l.strip_prefix("Copyright (c) "))
        .expect("LICENSE-MIT has a copyright line");
    let notice = std::fs::read_to_string(root.join("examples/coin_dash/CONTENT-NOTICES.txt")).unwrap();
    let lines: Vec<&str> = notice.lines().filter(|l| l.contains("Copyright (c)")).collect();
    assert!(!lines.is_empty());
    for line in lines {
        assert!(
            line.contains(holder.trim_end_matches('.')),
            "the notice's copyright line {line:?} differs from LICENSE-MIT's {holder:?}"
        );
    }
}
