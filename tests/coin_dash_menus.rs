//! The `coin_dash` reference game's menus, played headlessly with frames
//! (`Game::headless_frames`): the real input mapping (`read_input`), the
//! retained-UI settings screen and the HUD all run, with no window or GPU.
//! An input scenario presses keys and clicks the settings screen's buttons
//! by name, and checks the UI tree at every step: title, settings, back to
//! the title, a round, the pause screen, the title again, and the settings
//! closed with Escape.

#[path = "../examples/coin_dash/game.rs"]
mod game;

use game::*;
use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};
use std::cell::RefCell;
use std::rc::Rc;

/// A key tapped: down on `tick`, up on the next.
fn tap(scenario: InputScenario, tick: u64, key: Key) -> InputScenario {
    scenario
        .at(tick, InputEvent::Key(key, true))
        .at(tick + 1, InputEvent::Key(key, false))
}

/// What the player sees after one frame: the screen, and whether the
/// settings screen (retained UI) is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    screen: Screen,
    settings_ui: bool,
}

#[test]
fn coin_dash_menus_lead_into_play_and_back_with_the_ui_checked_at_each_step() {
    // ANCHOR: menus
    let settings = || ScenarioCheck::ui("settings");
    let mut scenario = InputScenario::new()
        // Title: the settings screen is not built yet.
        .expect(5, settings().exists(false));
    // O opens the settings: read_input sees it in frame 10, the rules
    // change the screen in tick 11, and the screen is pushed in frame 11.
    scenario = tap(scenario, 10, Key::O)
        .expect(11, settings().shown(true))
        .expect(
            11,
            ScenarioCheck::ui("settings.back")
                .shown(true)
                .text("Back")
                .focused(false),
        )
        .expect(11, ScenarioCheck::ui("settings.scale").text("100%"));
    // Tab twice: past "-" to "+".
    scenario = tap(scenario, 14, Key::Tab);
    scenario = tap(scenario, 16, Key::Tab).expect(16, ScenarioCheck::ui("settings.scale_up").focused(true));
    // Click "+": the UI scale goes up a step.
    scenario = scenario
        .click(20, "settings.scale_up")
        .expect(20, ScenarioCheck::ui("settings.scale").text("125%"))
        // "Back" (clicked in frame 25) leaves in tick 26.
        .click(25, "settings.back")
        .expect(25, settings().shown(true))
        .expect(26, settings().exists(true).shown(false));
    // Enter starts a round; Escape pauses it; Q goes back to the title.
    scenario = tap(scenario, 30, Key::Enter).expect(31, settings().shown(false));
    scenario = tap(scenario, 40, Key::Escape).expect(41, settings().shown(false));
    scenario = tap(scenario, 45, Key::Q);
    // The settings again, kept as they were; Escape closes them (the UI
    // pops the screen, the game leaves it).
    scenario = tap(scenario, 50, Key::O)
        .expect(51, settings().shown(true))
        .expect(51, ScenarioCheck::ui("settings.scale").text("125%"));
    scenario = tap(scenario, 55, Key::Escape)
        .expect(55, settings().shown(false))
        .expect(56, settings().shown(false))
        .duration(60)
        .named("coin dash menus");
    // ANCHOR_END: menus

    // Every frame, what was shown, for the screens the scenario can't name.
    let seen = Rc::new(RefCell::new(Vec::<(u64, Shown)>::new()));
    let log = seen.clone();
    let game = Game::new("coin dash menus", ARENA.x as u32, ARENA.y as u32)
        .seed(SEED)
        .headless_frames(true)
        .plugin(CoinDash)
        .system(move |ctx: &mut Ctx| {
            let shown = Shown {
                screen: screen(&ctx.world),
                settings_ui: ctx.ui.find("settings").is_some_and(|id| ctx.ui.is_shown(id)),
            };
            log.borrow_mut().push((ctx.time.sim_tick() - 1, shown));
        });
    let (world, report) = game.run_headless_scenario(scenario, |_| {});
    report.assert_passed();

    let at = |tick: u64| seen.borrow()[tick as usize].1;
    let shown = |screen, settings_ui| Shown { screen, settings_ui };
    assert_eq!(at(5), shown(Screen::Title, false));
    assert_eq!(at(11), shown(Screen::Settings, true));
    assert_eq!(at(26), shown(Screen::Title, false));
    assert_eq!(at(31), shown(Screen::Playing, false));
    assert_eq!(at(41), shown(Screen::Paused, false));
    assert_eq!(at(46), shown(Screen::Title, false));
    assert_eq!(at(51), shown(Screen::Settings, true));
    assert_eq!(at(56), shown(Screen::Title, false));
    // The screens in order, each once: nothing in between.
    let mut order: Vec<Screen> = Vec::new();
    for (_, s) in seen.borrow().iter() {
        if order.last() != Some(&s.screen) {
            order.push(s.screen);
        }
    }
    use Screen::*;
    assert_eq!(order, [Title, Settings, Title, Playing, Paused, Title, Settings, Title]);
    // The click on "+" changed the setting, and a round was started.
    assert_eq!(world.resource::<settings::Settings>().ui_scale, 1.25);
    assert_eq!(world.resource::<Round>().number, 1);
}

#[test]
fn coin_dash_menus_fade_in_on_the_ui_clock() {
    // O opens the settings in frame 26, Escape closes them in frame 40.
    let scenario = tap(tap(InputScenario::new(), 25, Key::O), 40, Key::Escape)
        .duration(70)
        .named("coin dash menu motion");
    // Every frame: the screen, how far its text has come in, and the
    // settings screen's opacity and whether it takes input.
    let seen = Rc::new(RefCell::new(Vec::<(Screen, f32, f32, bool)>::new()));
    let log = seen.clone();
    let game = Game::new("coin dash menu motion", ARENA.x as u32, ARENA.y as u32)
        .seed(SEED)
        .headless_frames(true)
        .plugin(CoinDash)
        .system(move |ctx: &mut Ctx| {
            let text = ctx.world.resource::<MenuFade>().shown;
            // Laid out as the frame's drawing lays it out, after the
            // systems that pushed or popped a screen.
            ctx.ui.layout(ARENA);
            let settings = ctx.ui.find("settings");
            let opacity = settings.and_then(|id| ctx.ui.drawn(id)).map_or(0.0, |d| d.opacity);
            let up = settings.is_some_and(|id| ctx.ui.is_shown(id));
            log.borrow_mut().push((screen(&ctx.world), text, opacity, up));
        });
    let (_, report) = game.run_headless_scenario(scenario, |_| {});
    report.assert_passed();
    let seen = seen.borrow();
    let fades_in = |values: &[f32]| {
        values.iter().any(|&v| v > 0.0 && v < 1.0)
            && values.windows(2).all(|w| w[1] >= w[0])
            && values.last() == Some(&1.0)
    };
    // The title's text fades and rises in.
    let title: Vec<f32> = seen.iter().take_while(|s| s.0 == Screen::Title).map(|s| s.1).collect();
    assert!(fades_in(&title), "the title text fades in: {title:?}");
    // The settings screen fades in while it already takes input...
    let open: Vec<f32> = (seen.iter())
        .skip_while(|s| s.0 != Screen::Settings)
        .take_while(|s| s.0 == Screen::Settings)
        .map(|s| s.2)
        .collect();
    assert!(fades_in(&open), "the settings screen fades in: {open:?}");
    // ...and fades out without input under the title's text coming back.
    let back = seen.iter().rposition(|s| s.0 == Screen::Settings).unwrap() + 1;
    let after = &seen[back..];
    assert!(after.iter().all(|s| s.0 == Screen::Title && !s.3));
    assert!(
        after.iter().any(|s| s.2 > 0.0 && s.2 < 1.0),
        "the settings fade out: {after:?}"
    );
    let title_again: Vec<f32> = after.iter().map(|s| s.1).collect();
    assert!(fades_in(&title_again), "the title text fades in again: {title_again:?}");
}
