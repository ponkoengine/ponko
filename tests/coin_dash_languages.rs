//! The `coin_dash` reference game in its languages: the string tables are
//! complete, every key the game asks for exists, the settings screen's
//! language button switches the game's text (into Arabic, right to left,
//! with the `text-shaping` feature) and the choice is saved, and the
//! pseudo-locale's longer text still fits the screen.

#[path = "../examples/coin_dash/game.rs"]
mod game;

use game::*;
use ponko::locale::Strings;
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};

/// The tables the game loads, as the game loads them.
fn tables() -> Strings {
    let mut strings = None;
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        for table in LANGUAGES {
            ctx.assets.load_strings(&mut ctx.world, table).unwrap();
        }
        strings = Some(ctx.world.resource::<Strings>().clone());
    });
    strings.unwrap()
}

#[test]
fn every_language_has_every_key_plural_form_and_argument() {
    let strings = tables();
    assert_eq!(strings.languages(), ["en", "ar"]);
    let report = strings.missing_report();
    assert!(report.is_empty(), "the string tables have gaps:\n{report}");
}

/// The string literals in the game's source that look like table keys
/// (`"settings.back"`), leaving out UI node names (`.name("...")`).
fn keys_in_source() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let prefixes = [
        "title.",
        "progress.",
        "paused.",
        "result.",
        "save.",
        "hud.",
        "settings.",
        "refused.",
        "control.",
    ];
    let mut keys = Vec::new();
    for file in ["examples/coin_dash/game.rs", "examples/coin_dash/settings.rs"] {
        let source = std::fs::read_to_string(root.join(file)).unwrap();
        for line in source
            .lines()
            .filter(|l| !l.contains(".name(") && !l.trim_start().starts_with("//"))
        {
            for literal in line.split('"').skip(1).step_by(2) {
                let is_key = prefixes.iter().any(|p| literal.starts_with(p))
                    && !literal.ends_with(".json")
                    && !literal.ends_with(".rs")
                    && literal.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_');
                if is_key {
                    keys.push(literal.to_string());
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

#[test]
fn every_key_the_game_asks_for_is_in_the_reference_table() {
    let strings = tables();
    let keys = keys_in_source();
    assert!(keys.len() > 30, "the scan found the keys: {keys:?}");
    let reference = strings.table(strings.reference()).unwrap();
    let missing: Vec<&String> = keys.iter().filter(|k| !reference.contains(k)).collect();
    assert!(missing.is_empty(), "keys the game uses but en.json lacks: {missing:?}");
}

/// Opens the settings (O), clicks the language button, goes back (Escape),
/// and returns the world, the frames' title lines being checked by the
/// scenario.
fn switch_language_on_the_settings_screen(data: &std::path::Path, expect_back: &str) -> World {
    let scenario = InputScenario::new()
        .at(10, ponko::actions::InputEvent::Key(Key::O, true))
        .at(11, ponko::actions::InputEvent::Key(Key::O, false))
        .expect(11, ScenarioCheck::ui("settings.back").shown(true).text("Back"))
        .expect(11, ScenarioCheck::ui("settings.language").text("English"))
        .click(15, "settings.language")
        // The click is read in frame 15; the labels follow from frame 16.
        .expect(16, ScenarioCheck::ui("settings.back").text(expect_back))
        .click(20, "settings.back")
        .duration(25)
        .named("coin dash language");
    let game = Game::new("coin dash languages", ARENA.x as u32, ARENA.y as u32)
        .seed(SEED)
        .data_dir(data)
        .headless_frames(true)
        .plugin(CoinDash);
    let (world, report) = game.run_headless_scenario(scenario, |_| {});
    report.assert_passed();
    world
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-coin-dash-lang-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(feature = "text-shaping")]
#[test]
fn the_language_button_switches_the_game_into_arabic_and_the_choice_is_kept() {
    let data = temp_dir("arabic");
    let world = switch_language_on_the_settings_screen(&data, "رجوع");
    assert_eq!(*world.resource::<Screen>(), Screen::Title);
    let strings = world.resource::<Strings>();
    assert_eq!(strings.language(), "ar");
    assert!(strings.is_rtl());
    let lines = screen_lines(&world);
    assert_eq!(lines[0].text, "سباق العملات");
    assert_eq!(
        lines[1].text, "اجمع ٨ عملات في ٢٠ ثانية",
        "Arabic plural form and digits"
    );
    assert!(
        lines.last().unwrap().text.contains("Enter: العب"),
        "{}",
        lines.last().unwrap().text
    );

    // Saved with the settings: the next run starts in Arabic.
    let saved = std::fs::read_to_string(data.join("settings.json")).unwrap();
    assert!(saved.contains("\"language\": \"ar\""), "{saved}");
    let mut language = String::new();
    Game::new("t", 1, 1)
        .data_dir(&data)
        .plugin(CoinDash)
        .run_headless(0, |ctx| {
            language = ctx.world.resource::<Strings>().language().to_string()
        });
    assert_eq!(language, "ar");
}

#[cfg(not(feature = "text-shaping"))]
#[test]
fn without_text_shaping_arabic_is_not_offered() {
    let data = temp_dir("no-shaping");
    let world = switch_language_on_the_settings_screen(&data, "Back");
    assert_eq!(world.resource::<Strings>().language(), "en");
    assert_eq!(screen_lines(&world)[0].text, "COIN DASH");
}

#[test]
fn a_saved_language_the_game_does_not_have_falls_back_to_english() {
    let data = temp_dir("unknown");
    let mut settings = settings::Settings::default();
    settings.language = "xx".into();
    std::fs::write(data.join("settings.json"), serde_json::to_string(&settings).unwrap()).unwrap();
    let mut seen = (String::new(), None);
    Game::new("t", 1, 1)
        .data_dir(&data)
        .plugin(CoinDash)
        .run_headless(0, |ctx| {
            seen = (
                ctx.world.resource::<Strings>().language().to_string(),
                Some(ctx.world.resource::<settings::SettingsFile>().load),
            );
        });
    assert_eq!(seen.0, "en");
    assert_eq!(seen.1, Some(LoadStatus::Loaded), "the file itself is fine");
}

/// The widest line of every screen with text (title, pause, both results),
/// in the current language, at `scale`, in window pixels.
fn widest_line(ctx: &mut Ctx, scale: f32) -> (f32, String) {
    let font = ctx.world.resource::<Strings>().font().unwrap_or(Font::DEFAULT);
    let mut widest = (0.0, String::new());
    for screen in [
        Screen::Title,
        Screen::Paused,
        Screen::Result(Outcome::Won),
        Screen::Result(Outcome::Lost),
    ] {
        ctx.world.insert_resource(screen);
        ctx.world.resource_mut::<Round>().new_best = true;
        for line in screen_lines(&ctx.world) {
            let width = ctx.gfx.measure_text_ex(&line.text, font, line.size * scale).x;
            if width > widest.0 {
                widest = (width, line.text);
            }
        }
    }
    widest
}

#[test]
fn the_pseudo_locales_longer_text_still_fits_the_arena() {
    Game::new("t", ARENA.x as u32, ARENA.y as u32)
        .plugin(CoinDash)
        .run_headless(0, |ctx| {
            // Played rounds make the progress line the longest.
            let progress = ctx.world.resource_mut::<Progress>();
            progress.rounds_played = 12;
            progress.rounds_won = 10;
            progress.best_time = Some(12.5);
            progress.best_coins = COINS;
            let (english, _) = widest_line(ctx, 1.0);
            assert!(ctx.world.resource_mut::<Strings>().set_language(Strings::PSEUDO));
            let (pseudo, text) = widest_line(ctx, 1.0);
            // Key names and numbers stay as they are, so a line grows less than a third.
            assert!(
                pseudo > english * 1.1,
                "the pseudo-locale is longer: {pseudo} vs {english}"
            );
            assert!(pseudo < ARENA.x - 24.0, "\"{text}\" is {pseudo} px wide");
        });
}
