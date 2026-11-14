//! Localization: string tables loaded from files (and reloaded), arguments,
//! plural rules, the missing-key report, the pseudo-locale, and retained UI
//! labels that follow the language.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ponko::locale::{
    Arg, Missing, MissingKind, PluralCategory, StringTable, Strings, plural_categories, plural_category,
};
use ponko::prelude::*;

const EN: &str = r#"{
  "language": "en",
  "name": "English",
  "strings": {
    "menu.play": "Play",
    "menu.quit": "Quit",
    "hud.coins": { "one": "{count} coin", "other": "{count} coins" },
    "hud.time": "Time {seconds:1} s"
  }
}"#;

const AR: &str = r#"{
  "language": "ar",
  "name": "العربية",
  "decimal": "٫",
  "digits": "٠١٢٣٤٥٦٧٨٩",
  "strings": {
    "menu.play": "العب",
    "hud.coins": {
      "zero": "لا عملات", "one": "عملة واحدة", "two": "عملتان",
      "few": "{count} عملات", "many": "{count} عملة", "other": "{count} عملة"
    },
    "hud.time": "الوقت {seconds:1} ث"
  }
}"#;

fn tables() -> Strings {
    let mut strings = Strings::default();
    strings.insert(StringTable::from_json(EN).unwrap());
    strings.insert(StringTable::from_json(AR).unwrap());
    strings
}

fn count(n: i64) -> [(&'static str, Arg); 1] {
    [("count", Arg::from(n))]
}

#[test]
fn the_first_table_is_the_reference_and_the_current_language() {
    let strings = tables();
    assert_eq!(strings.languages(), ["en", "ar"]);
    assert_eq!(strings.reference(), "en");
    assert_eq!(strings.language(), "en");
    assert_eq!(strings.table("ar").unwrap().name(), "العربية");
    assert!(!strings.is_rtl());
}

#[test]
fn keys_resolve_in_the_current_language_then_the_reference_then_as_the_key() {
    let mut strings = tables();
    assert!(strings.set_language("ar"));
    assert!(strings.is_rtl());
    assert_eq!(strings.get("menu.play"), "العب");
    assert_eq!(strings.get("menu.quit"), "Quit");
    assert_eq!(strings.get("menu.nothing"), "menu.nothing");
    assert!(!strings.contains("menu.nothing"));
}

#[test]
fn an_unknown_language_keeps_the_current_one() {
    let mut strings = tables();
    let before = strings.revision();
    assert!(!strings.set_language("xx"));
    assert_eq!(strings.language(), "en");
    assert_eq!(strings.revision(), before, "nothing changed");
    assert!(strings.set_language("ar"));
    assert_ne!(strings.revision(), before);
}

#[test]
fn the_count_argument_picks_the_plural_form_by_the_languages_rules() {
    let mut strings = tables();
    assert_eq!(strings.format("hud.coins", &count(1)), "1 coin");
    assert_eq!(strings.format("hud.coins", &count(0)), "0 coins");
    assert_eq!(strings.format("hud.coins", &count(21)), "21 coins");
    strings.set_language("ar");
    let ar = |n| strings.format("hud.coins", &count(n));
    assert_eq!(ar(0), "لا عملات");
    assert_eq!(ar(1), "عملة واحدة");
    assert_eq!(ar(2), "عملتان");
    assert_eq!(ar(3), "٣ عملات", "Arabic digits from the table");
    assert_eq!(ar(11), "١١ عملة");
    assert_eq!(ar(100), "١٠٠ عملة");
}

#[test]
fn numbers_use_the_placeholders_decimals_and_the_tables_separators() {
    let mut strings = tables();
    let seconds = [("seconds", Arg::from(12.25f32))];
    assert_eq!(strings.format("hud.time", &seconds), "Time 12.2 s");
    strings.set_language("ar");
    assert_eq!(strings.format("hud.time", &seconds), "الوقت ١٢٫٢ ث");
}

#[test]
fn format_into_reuses_the_string() {
    let strings = tables();
    let mut out = String::from("old text");
    strings.format_into("hud.coins", &count(5), &mut out);
    assert_eq!(out, "5 coins");
}

#[test]
fn plural_rules_follow_cldr_for_the_built_in_languages() {
    use PluralCategory::*;
    let cases: &[(&str, f64, PluralCategory)] = &[
        ("en", 1.0, One),
        ("en", 1.5, Other),
        ("de-AT", 1.0, One),
        ("fr", 0.0, One),
        ("fr", 1.5, One),
        ("fr", 2.0, Other),
        ("pt-BR", 0.0, One),
        ("pt-PT", 0.0, Other),
        ("ru", 1.0, One),
        ("ru", 11.0, Many),
        ("ru", 21.0, One),
        ("ru", 22.0, Few),
        ("ru", 25.0, Many),
        ("ru", 1.5, Other),
        ("uk", 104.0, Few),
        ("pl", 1.0, One),
        ("pl", 12.0, Many),
        ("pl", 102.0, Few),
        ("pl", 21.0, Many),
        ("cs", 3.0, Few),
        ("cs", 5.0, Other),
        ("cs", 1.5, Many),
        ("hr", 21.0, One),
        ("hr", 5.0, Other),
        ("ar", 0.0, Zero),
        ("ar", 2.0, Two),
        ("ar", 103.0, Few),
        ("ar", 111.0, Many),
        ("ar", 100.0, Other),
        ("he", 2.0, Two),
        ("he", 20.0, Other),
        ("ro", 0.0, Few),
        ("ro", 20.0, Other),
        ("lt", 21.0, One),
        ("lt", 12.0, Other),
        ("lv", 0.0, Zero),
        ("lv", 31.0, One),
        ("ja", 1.0, Other),
        ("zh-Hant", 1.0, Other),
    ];
    for &(language, n, want) in cases {
        assert_eq!(plural_category(language, n), want, "{language} {n}");
        assert!(plural_categories(language).contains(&want), "{language} lists {want:?}");
    }
}

#[test]
fn missing_lists_keys_plural_forms_and_arguments_a_language_lacks() {
    let mut strings = tables();
    strings.insert(
        StringTable::from_json(
            r#"{"language": "ru", "strings": {
                "menu.play": "Играть", "menu.quit": "Выйти",
                "hud.coins": {"one": "{count} монета", "other": "{count} монет"},
                "hud.time": "Время"}}"#,
        )
        .unwrap(),
    );
    let gaps = |language| -> Vec<(String, MissingKind)> {
        strings
            .missing(language)
            .into_iter()
            .map(|m: Missing| (m.key, m.what))
            .collect()
    };
    assert_eq!(gaps("en"), []);
    assert_eq!(gaps("ar"), [("menu.quit".to_string(), MissingKind::Key)]);
    assert_eq!(
        gaps("ru"),
        [
            ("hud.coins".to_string(), MissingKind::PluralForm(PluralCategory::Few)),
            ("hud.coins".to_string(), MissingKind::PluralForm(PluralCategory::Many)),
            ("hud.time".to_string(), MissingKind::Argument("seconds".into())),
        ]
    );
    let report = strings.missing_report();
    assert!(report.contains("ar: \"menu.quit\" is missing"), "{report}");
    assert!(
        report.contains("ru: \"hud.coins\" has no \"few\" plural form"),
        "{report}"
    );
    assert!(report.contains("ru: \"hud.time\" doesn't use {seconds}"), "{report}");
    assert_eq!(report.lines().count(), 4);
}

#[test]
fn the_pseudo_locale_accents_and_lengthens_the_reference_text() {
    let mut strings = tables();
    strings.set_language("ar");
    assert!(strings.set_language(Strings::PSEUDO));
    assert_eq!(strings.language(), Strings::PSEUDO);
    assert!(!strings.is_rtl());
    let play = strings.get("menu.play");
    assert_eq!(play, "[Ƥļàý··]");
    let coins = strings.format("hud.coins", &count(2));
    assert!(coins.starts_with("[2 ") && coins.ends_with(']'), "{coins}");
    assert!(!coins.contains("coins"), "letters are accented: {coins}");
    assert_eq!(strings.get("menu.nothing"), "menu.nothing", "unknown keys stay plain");
    assert!(strings.set_language("en"));
    assert_eq!(strings.get("menu.play"), "Play");
}

#[test]
fn bad_tables_say_what_is_wrong() {
    let err = |json: &str| StringTable::from_json(json).unwrap_err().to_string();
    assert!(err(r#"{"strings": {}}"#).contains("\"language\""));
    assert!(err(r#"{"language": "en", "strings": {"a": 3}}"#).contains("\"a\""));
    assert!(err(r#"{"language": "en", "strings": {"a": {"lots": "x"}}}"#).contains("\"lots\" is not a plural form"));
    assert!(err(r#"{"language": "en", "digits": "0123"}"#).contains("ten characters"));
    assert!(err(r#"{"language": "en", "direction": "up"}"#).contains("direction"));
    assert_eq!(StringTable::from_json("[1]").unwrap_err().kind(), ErrorKind::Decode);
}

fn temp_file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-locale-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn load_strings_adds_the_resource_and_reports_a_missing_file() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("built-in")
                .with("lang/en.json", EN)
                .with("lang/ar.json", AR),
        );
        ctx.assets.load_strings(&mut ctx.world, "lang/en.json").unwrap();
        ctx.assets.load_strings(&mut ctx.world, "lang/ar.json").unwrap();
        assert_eq!(ctx.world.resource::<Strings>().languages(), ["en", "ar"]);
        let err = ctx.assets.load_strings(&mut ctx.world, "lang/xx.json").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    });
}

/// Loads `path`, edits it on tick 2 with `edit`, and returns `menu.play`'s
/// text on every tick.
fn run_reloading(path: &Path, edit: impl Fn() + 'static) -> Vec<String> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let name = path.to_str().unwrap().to_string();
    Game::new("hot", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            log.lock()
                .unwrap()
                .push(ctx.world.resource::<Strings>().get("menu.play").into_owned());
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(Duration::from_millis(50));
                edit();
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .run_headless(5, move |ctx| ctx.assets.load_strings(&mut ctx.world, &name).unwrap());
    seen.lock().unwrap().clone()
}

#[test]
fn an_edited_table_is_reloaded_and_a_broken_one_keeps_the_old_text() {
    let path = temp_file("en.json", EN);
    let p = path.clone();
    let seen = run_reloading(&path, move || {
        std::fs::write(&p, EN.replace("\"Play\"", "\"Start\"")).unwrap()
    });
    assert_eq!(seen, ["Play", "Play", "Play", "Start", "Start"]);

    let path = temp_file("en-broken.json", EN);
    let p = path.clone();
    let seen = run_reloading(&path, move || std::fs::write(&p, "{ not json").unwrap());
    assert!(seen.iter().all(|t| t == "Play"), "{seen:?}");
}

#[test]
fn keyed_labels_follow_the_language_from_the_next_frame() {
    let texts = Arc::new(Mutex::new(Vec::new()));
    let log = texts.clone();
    Game::new("t", 320, 240)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let label = ctx.ui.find("coins").unwrap();
            log.lock().unwrap().push(ctx.ui.text(label).unwrap().to_string());
            if ctx.time.frame() == 1 {
                ctx.world.resource_mut::<Strings>().set_language("ar");
            }
        })
        .run_headless(3, |ctx| {
            ctx.world.insert_resource(tables());
            let root = ctx.ui.root().id();
            let label = ctx.ui.label(root, "").name("coins").id();
            ctx.ui.set_text_key(label, "hud.coins", &count(2));
            ctx.ui.push_screen(root, false);
        });
    let texts = texts.lock().unwrap().clone();
    assert_eq!(texts.first().map(String::as_str), Some("2 coins"));
    assert_eq!(texts.last().map(String::as_str), Some("عملتان"));
}

#[test]
fn a_keyed_node_uses_the_languages_font_and_its_own_without_one() {
    let mut strings = tables();
    let mut other = None;
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        other = Some(ctx.gfx.load_font("fonts/NotoSansArabic-Subset.ttf"));
    });
    let other = other.unwrap();
    let mut ui = ponko::ui::Ui::default();
    let root = ui.root().id();
    let label = ui.label(root, "").text_key("menu.play").id();
    let own = ui.paint(label).unwrap().font;
    assert_ne!(other, own);
    ui.localize(&strings);
    assert_eq!(ui.text(label), Some("Play"));

    strings.set_font("ar", other);
    strings.set_language("ar");
    ui.localize(&strings);
    assert_eq!(ui.paint(label).unwrap().font, other);
    assert_eq!(ui.text(label), Some("العب"));
    strings.set_language("en");
    ui.localize(&strings);
    assert_eq!(ui.paint(label).unwrap().font, own);

    // Plain text lets go of the key: it no longer follows.
    ui.set_text(label, "Fixed");
    strings.set_language("ar");
    ui.localize(&strings);
    assert_eq!(ui.text(label), Some("Fixed"));
}

#[test]
fn a_tables_font_file_is_loaded_for_its_language_with_the_built_in_font_behind_it() {
    let seen = Arc::new(Mutex::new(None));
    let log = seen.clone();
    let loaded = Arc::new(Mutex::new(None));
    let handle = loaded.clone();
    Game::new("t", 320, 240)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            // Frames count from 1.
            if ctx.time.frame() == 1 {
                ctx.world.resource_mut::<Strings>().set_language("ar");
            } else {
                let play = ctx.ui.find("play").unwrap();
                let font = ctx.ui.paint(play).unwrap().font;
                *log.lock().unwrap() = Some((
                    font,
                    ctx.world.resource::<Strings>().font(),
                    ctx.gfx.font_fallbacks(font).to_vec(),
                    ctx.ui.text(play).unwrap().to_string(),
                ));
            }
        })
        .run_headless(3, move |ctx| {
            let ar = AR.replace("\"name\"", "\"font\": \"fonts/NotoSansArabic-Subset.ttf\", \"name\"");
            ctx.assets
                .add_source(MemorySource::new("built-in").with("en.json", EN).with("ar.json", ar));
            ctx.assets.load_strings(&mut ctx.world, "en.json").unwrap();
            ctx.assets.load_strings(&mut ctx.world, "ar.json").unwrap();
            assert_eq!(
                ctx.world.resource::<Strings>().table("ar").unwrap().font_path(),
                Some("fonts/NotoSansArabic-Subset.ttf")
            );
            let root = ctx.ui.root().id();
            ctx.ui.label(root, "").text_key("menu.play").name("play");
            ctx.ui.push_screen(root, false);
            *handle.lock().unwrap() = Some(ctx.gfx.load_font("fonts/NotoSansArabic-Subset.ttf"));
        });
    let arabic = loaded.lock().unwrap().unwrap();
    let (font, strings_font, fallbacks, text) = seen.lock().unwrap().clone().expect("frame 1 ran");
    assert_eq!(font, arabic, "the label is drawn with the table's font");
    assert_eq!(strings_font, Some(arabic));
    assert_eq!(fallbacks, [Font::DEFAULT], "Latin text inside Arabic still draws");
    assert_eq!(text, "العب");
}
