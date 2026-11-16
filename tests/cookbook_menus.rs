//! Cookbook, "Menus and screens": a pause menu, a title screen with a button,
//! game over and restart, a HUD, a volume slider and a sliding panel. Every
//! recipe between `ANCHOR` lines is what `book/src/cookbook/menus.md` shows,
//! and each test runs it headless (with frames, so the UI runs) for a few
//! ticks and checks that it did its job.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};
use ponko::ui::ScreenMode;
use serde::{Deserialize, Serialize};

/// Ticks the simulation ran in some state, so tests can see whether it was paused.
#[derive(Default)]
struct Steps(Vec<u64>);

// ANCHOR: pause
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen {
    Playing,
    Paused,
}

fn add_pause_menu(game: ConfiguredGame) -> ConfiguredGame {
    // Up while paused. Resume, like Escape, closes it, and that leaves the state.
    game.state_screen(Screen::Paused, ScreenMode::Modal, |ctx| {
        let menu = ctx.ui.root().layout(UiLayout::centered(300.0, 160.0)).id();
        ctx.ui.title(menu, "Paused");
        ctx.ui.button(menu, "Resume").focused().closes_screen();
        menu
    })
    .system(|ctx: &mut Ctx| {
        if ctx.input.key_pressed(Key::Escape) && ctx.state::<Screen>() == &Screen::Playing {
            ctx.push_state(Screen::Paused);
        }
    })
}
// ANCHOR_END: pause

fn pausable_game() -> ConfiguredGame {
    Game::new("pause", 800, 600)
        .state(Screen::Playing)
        .setup(|ctx| ctx.world.insert_resource(Steps::default()))
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| {
                let tick = ctx.time.sim_tick();
                ctx.world.resource_mut::<Steps>().0.push(tick);
            })
            .run_if(in_state(Screen::Playing)),
        )
}

#[test]
fn escape_pauses_the_world_and_resume_or_escape_brings_it_back() {
    /// Whether a screen was up, per frame.
    #[derive(Default)]
    struct MenuUp(Vec<(u64, bool)>);
    let tap = |s: InputScenario, key: Key, tick: u64| {
        s.at(tick, InputEvent::Key(key, true))
            .at(tick + 1, InputEvent::Key(key, false))
    };
    // Escape pauses at 10; Enter presses Resume (it has the focus) at 20.
    // Escape pauses at 30 and closes the menu at 40.
    let scenario = InputScenario::new();
    let scenario = tap(
        tap(tap(tap(scenario, Key::Escape, 10), Key::Enter, 20), Key::Escape, 30),
        Key::Escape,
        40,
    );
    let (world, report) = add_pause_menu(pausable_game().setup(|ctx| ctx.world.insert_resource(MenuUp::default())))
        .system(|ctx: &mut Ctx| {
            let entry = (ctx.time.sim_tick(), ctx.ui.top_screen().is_some());
            ctx.world.resource_mut::<MenuUp>().0.push(entry);
        })
        .headless_frames(true)
        .run_headless_scenario(scenario.duration(50));
    report.assert_passed();
    let steps = &world.resource::<Steps>().0;
    let ran = |from: u64, to: u64| steps.iter().filter(|&&t| (from..to).contains(&t)).count();
    assert_eq!(ran(0, 10), 10, "playing");
    assert_eq!(ran(13, 20), 0, "paused: nothing ran");
    assert_eq!(ran(24, 30), 6, "resumed with the button");
    assert_eq!(ran(33, 40), 0, "paused again");
    assert_eq!(ran(44, 50), 6, "resumed with Escape");
    let up = |tick: u64| world.resource::<MenuUp>().0.iter().any(|&(t, up)| t == tick && up);
    assert!(
        !up(5) && up(15) && !up(25) && up(35) && !up(45),
        "the menu is up exactly while paused"
    );
}

// ANCHOR: title
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Page {
    Title,
    InGame,
}

fn build_title(ctx: &mut Ctx) {
    let menu = ctx.ui.root().layout(UiLayout::centered(300.0, 200.0).gap(12.0)).id();
    ctx.ui.title(menu, "Treasure Dash").layout(UiLayout::row(48.0));
    ctx.ui
        .button(menu, "Play")
        .layout(UiLayout::row(44.0))
        .name("title.play")
        .focused();
    ctx.ui
        .button(menu, "Quit")
        .layout(UiLayout::row(44.0))
        .name("title.quit");
    ctx.ui.push_modeless(menu);
}

/// A frame system: what the buttons do. Clicks are events, read once.
fn title_buttons(ctx: &mut Ctx) {
    let (play, quit) = (ctx.ui.find("title.play"), ctx.ui.find("title.quit"));
    let clicks: Vec<UiId> = ctx.ui.events().filter_map(UiEvent::clicked).collect();
    for click in clicks {
        if Some(click) == play {
            ctx.ui.pop_screen(); // close the title
            ctx.set_state(Page::InGame);
        } else if Some(click) == quit {
            ctx.quit();
        }
    }
}
// ANCHOR_END: title

#[test]
fn the_play_button_closes_the_title_and_starts_the_game() {
    let scenario = InputScenario::new()
        .expect(0, ScenarioCheck::ui("title.play").shown(true).focused(true))
        .click(3, "title.play")
        .expect(4, ScenarioCheck::ui("title.play").shown(false))
        .duration(8);
    let (world, report) = Game::new("title", 800, 600)
        .state(Page::Title)
        .setup(build_title)
        .system(title_buttons)
        .headless_frames(true)
        .run_headless_scenario(scenario);
    report.assert_passed();
    assert_eq!(*world.resource::<ponko::state::State<Page>>().get(), Page::InGame);
}

#[test]
fn the_quit_button_ends_the_run_early() {
    let scenario = InputScenario::new().click(3, "title.quit").duration(50);
    let (world, report) = Game::new("title", 800, 600)
        .state(Page::Title)
        .setup(build_title)
        .setup(|ctx| ctx.world.insert_resource(Vec::<(u64, bool)>::new()))
        .system(title_buttons)
        .system(|ctx: &mut Ctx| {
            let entry = (ctx.time.sim_tick(), ctx.quit_requested());
            ctx.world.resource_mut::<Vec<(u64, bool)>>().push(entry);
        })
        .headless_frames(true)
        .run_headless_scenario(scenario);
    let seen = world.resource::<Vec<(u64, bool)>>();
    assert_eq!(seen.last(), Some(&(4, true)), "the click on tick 3 asked to quit");
    assert_eq!(report.ticks_run, 4, "and no tick ran after it");
}

// ANCHOR: game_over
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Round {
    Playing,
    GameOver,
}

struct Lives(u32);

fn lose_a_life(ctx: &mut Ctx) {
    let lives = ctx.world.resource_mut::<Lives>();
    lives.0 = lives.0.saturating_sub(1);
    if lives.0 == 0 {
        ctx.set_state(Round::GameOver); // applies at the end of the tick
    }
}

/// On the game over screen, Enter plays again.
fn play_again(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Enter) {
        // Setting the state it is already in isn't a no-op: it leaves and
        // enters `Playing`, so the on-enter work below runs again.
        ctx.set_state(Round::Playing);
    }
}

fn add_rounds(game: Game) -> Game {
    game.state(Round::Playing)
        .on_enter(Round::Playing, |ctx: &mut Ctx| ctx.world.insert_resource(Lives(3)))
        .add(Phase::AfterPhysics, play_again.run_if(in_state(Round::GameOver)))
}
// ANCHOR_END: game_over

#[test]
fn losing_every_life_ends_the_round_and_enter_starts_a_new_one() {
    #[derive(Default)]
    struct Seen(Vec<(u64, Round, u32)>);
    let mut game = add_rounds(Game::new("rounds", 800, 600)).setup(|ctx| ctx.world.insert_resource(Seen::default()));
    game = game
        .sim_system(|ctx: &mut Ctx| {
            // The test loses a life on ticks 4, 5 and 6.
            if (4..=6).contains(&ctx.time.sim_tick()) && *ctx.state::<Round>() == Round::Playing {
                lose_a_life(ctx);
            }
        })
        .sim_system(|ctx: &mut Ctx| {
            let entry = (
                ctx.time.sim_tick(),
                *ctx.state::<Round>(),
                ctx.world.resource::<Lives>().0,
            );
            ctx.world.resource_mut::<Seen>().0.push(entry);
        })
        .input_scenario(
            InputScenario::new()
                .at(12, InputEvent::Key(Key::Enter, true))
                .at(13, InputEvent::Key(Key::Enter, false)),
        );
    let world = game.run_headless(20);
    let seen = &world.resource::<Seen>().0;
    assert_eq!(seen[2], (2, Round::Playing, 3));
    assert_eq!(seen[8], (8, Round::GameOver, 0), "out of lives");
    assert_eq!(seen[19], (19, Round::Playing, 3), "playing again with fresh lives");
}

// ANCHOR: hud
fn build_hud(ctx: &mut Ctx) {
    let corner = ctx.ui.root().layout(UiLayout::fill().padding(16.0)).id();
    ctx.ui.label(corner, "Score: 0").name("hud.score");
    ctx.ui.show_hud(corner); // always shown, never takes clicks
}

#[derive(Default)]
struct Score(u32);

fn update_hud(ctx: &mut Ctx) {
    let text = format!("Score: {}", ctx.world.resource::<Score>().0);
    if let Some(label) = ctx.ui.find("hud.score")
        && ctx.ui.text(label) != Some(&text)
    {
        ctx.ui.set_text(label, text);
    }
}
// ANCHOR_END: hud

#[test]
fn the_hud_label_follows_the_score() {
    let scenario = InputScenario::new()
        .expect(2, ScenarioCheck::ui("hud.score").text("Score: 0"))
        .expect(8, ScenarioCheck::ui("hud.score").text("Score: 30"))
        .duration(10);
    let (_world, report) = Game::new("hud", 800, 600)
        .setup(|ctx| {
            ctx.world.insert_resource(Score::default());
            build_hud(ctx);
        })
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 5 {
                ctx.world.resource_mut::<Score>().0 += 30;
            }
        })
        .system(update_hud)
        .headless_frames(true)
        .run_headless_scenario(scenario);
    report.assert_passed();
}

// ANCHOR: volume_slider
struct MusicBus(AudioBus);

fn build_volume_slider(ctx: &mut Ctx) {
    let panel = ctx
        .ui
        .root()
        .layout(UiLayout::anchored(0.0, 0.0, 0.3, 0.1).padding(8.0))
        .id();
    ctx.ui
        .slider(panel)
        .layout(UiLayout::row(24.0))
        .value(0.8)
        .name("options.volume");
    ctx.ui.push_modeless(panel);
    let music = ctx.audio.bus("music"); // a named bus: everything played on it shares one volume
    ctx.world.insert_resource(MusicBus(music));
}

/// A frame system: the slider's value (0 to 1) is the bus's volume.
fn apply_volume(ctx: &mut Ctx) {
    let Some(slider) = ctx.ui.find("options.volume") else {
        return;
    };
    let volume = ctx.ui.value(slider).unwrap_or(1.0);
    let music = ctx.world.resource::<MusicBus>().0;
    ctx.audio.set_bus_volume(music, volume);
}
// ANCHOR_END: volume_slider

#[test]
fn the_slider_sets_the_music_bus_volume() {
    #[derive(Default)]
    struct Heard(Vec<f32>);
    let (world, report) = Game::new("volume", 800, 600)
        .setup(|ctx| {
            ctx.world.insert_resource(Heard::default());
            build_volume_slider(ctx);
        })
        .system(|ctx: &mut Ctx| {
            // The test moves the slider on frame 4 (a player would drag it).
            if ctx.time.frame() == 4 {
                let slider = ctx.ui.find("options.volume").unwrap();
                ctx.ui.set_value(slider, 0.25);
            }
        })
        .system(apply_volume)
        .system(|ctx: &mut Ctx| {
            let music = ctx.world.resource::<MusicBus>().0;
            let volume = ctx.audio.bus_volume(music);
            ctx.world.resource_mut::<Heard>().0.push(volume);
        })
        .headless_frames(true)
        .run_headless_scenario(InputScenario::new().duration(8));
    report.assert_passed();
    let heard = &world.resource::<Heard>().0;
    assert_eq!(heard[2], 0.8);
    assert_eq!(heard[7], 0.25);
}

// ANCHOR: slide_in
#[derive(Default)]
struct SlideIn(f32); // seconds since the panel was asked to appear

fn build_side_panel(ctx: &mut Ctx) {
    let panel = ctx
        .ui
        .root()
        .layout(UiLayout::fixed(240.0, 160.0).top(40.0).left(-240.0))
        .id();
    ctx.ui.label(panel, "Inventory").name("side.title");
    ctx.ui.set_name(panel, "side.panel");
    ctx.ui.show_hud(panel);
}

/// A frame system: 0.4 seconds to slide in from off the left edge, easing out.
fn slide_in_panel(ctx: &mut Ctx) {
    let seconds = &mut ctx.world.resource_mut::<SlideIn>().0;
    *seconds += ctx.time.frame_dt();
    let t = (*seconds / 0.4).min(1.0);
    let x = -240.0 * (1.0 - t).powi(3); // from -240 to 0, slowing down
    let panel = ctx.ui.find("side.panel").unwrap();
    ctx.ui
        .set_layout(panel, UiLayout::fixed(240.0, 160.0).top(40.0).left(x));
}
// ANCHOR_END: slide_in

#[test]
fn the_panel_slides_in_from_the_left_and_stops_at_the_edge() {
    #[derive(Default)]
    struct Left(Vec<f32>);
    let (world, report) = Game::new("slide", 800, 600)
        .setup(|ctx| {
            ctx.world.insert_resource(SlideIn::default());
            ctx.world.insert_resource(Left::default());
            build_side_panel(ctx);
        })
        .system(slide_in_panel)
        .system(|ctx: &mut Ctx| {
            ctx.ui.layout(ctx.window_size()); // the layout the next frame will start from
            let panel = ctx.ui.find("side.panel").unwrap();
            let x = ctx.ui.rect(panel).unwrap().min.x;
            ctx.world.resource_mut::<Left>().0.push(x);
        })
        .headless_frames(true)
        .run_headless_scenario(InputScenario::new().duration(60));
    report.assert_passed();
    let left = &world.resource::<Left>().0;
    assert!(left[0] < -200.0, "starts off screen: {}", left[0]);
    assert!(left.windows(2).all(|w| w[1] >= w[0]), "only ever moves right");
    assert_eq!(*left.last().unwrap(), 0.0, "ends at the edge");
}
