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
