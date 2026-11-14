//! Localization through the C API (#411): string tables, the language,
//! arguments and plural forms, the missing-key report, fonts per language
//! and UI text that holds a key.
use ponko_ffi::*;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const EN: &CStr = cr#"{"language": "en", "name": "English", "strings": {
    "play": "Play",
    "coins": {"one": "{count} coin", "other": "{count} coins"},
    "time": "Time {seconds:1} s",
    "hello": "Hello, {name}"}}"#;
const AR: &CStr = cr#"{"language": "ar", "name": "العربية", "digits": "٠١٢٣٤٥٦٧٨٩", "strings": {
    "play": "العب",
    "coins": {"zero": "لا عملات", "one": "عملة", "two": "عملتان", "few": "{count} عملات", "many": "{count} عملة", "other": "{count} عملة"},
    "time": "الوقت {seconds:1} ث"}}"#;

fn run(system: PonkoSystemFn, user: *mut c_void) {
    unsafe {
        let game = ponko_game_new(c"locale".as_ptr(), 1, 1);
        assert_eq!(ponko_game_sim_system(game, c"s".as_ptr(), 1, system, user), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

/// Reads a `(buffer, capacity, out_len)` output into a String.
unsafe fn read(mut f: impl FnMut(*mut c_char, usize, *mut usize) -> PonkoStatus) -> String {
    let mut len = 0;
    assert_eq!(f(null_mut(), 0, &mut len), PONKO_OK);
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(f(buffer.as_mut_ptr().cast(), buffer.len(), &mut len), PONKO_OK);
    String::from_utf8(buffer[..len].to_vec()).unwrap()
}

fn int(name: &CStr, n: i64) -> PonkoStringArg {
    PonkoStringArg {
        name: name.as_ptr(),
        kind: PonkoStringArgKind::PONKO_STRING_ARG_INT,
        text: null(),
        integer: n,
        number: 0.0,
    }
}

#[derive(Default)]
struct Seen {
    before_tables: Option<PonkoStatus>,
    languages: Vec<(String, String)>,
    en: Vec<String>,
    ar: Vec<String>,
    rtl: bool,
    unknown_kept: bool,
    report: String,
    label_en: String,
    label_ar: String,
    label_plain: String,
    font_has: bool,
    font_matches: bool,
    load_missing: Option<PonkoStatus>,
}

unsafe extern "C" fn system(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Seen>() };
    unsafe {
        let mut known = false;
        seen.before_tables = Some(ponko_ctx_strings_set_language(ctx, c"en".as_ptr(), &mut known));
        assert_eq!(ponko_ctx_strings_insert_json(ctx, EN.as_ptr()), PONKO_OK);
        assert_eq!(ponko_ctx_strings_insert_json(ctx, AR.as_ptr()), PONKO_OK);
        assert_eq!(
            ponko_ctx_strings_insert_json(ctx, c"{\"strings\": {}}".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        seen.load_missing = Some(ponko_ctx_load_strings(ctx, c"lang/none.json".as_ptr()));

        let mut n = 0;
        assert_eq!(ponko_ctx_strings_language_count(ctx, &mut n), PONKO_OK);
        for i in 0..n {
            let mut ignored = 0;
            let code = read(|b, c, l| ponko_ctx_strings_language_at(ctx, i, b, c, l, null_mut(), 0, &mut ignored));
            let name = read(|b, c, l| ponko_ctx_strings_language_at(ctx, i, null_mut(), 0, &mut ignored, b, c, l));
            seen.languages.push((code, name));
        }
        let mut ignored = 0;
        assert_eq!(
            ponko_ctx_strings_language_at(ctx, n, null_mut(), 0, &mut ignored, null_mut(), 0, &mut ignored),
            PONKO_NOT_FOUND
        );

        let format = |key: &CStr, args: &[PonkoStringArg]| {
            read(|b, c, l| ponko_ctx_strings_format(ctx, key.as_ptr(), args.as_ptr(), args.len(), b, c, l))
        };
        let seconds = PonkoStringArg {
            name: c"seconds".as_ptr(),
            kind: PonkoStringArgKind::PONKO_STRING_ARG_NUMBER,
            text: null(),
            integer: 0,
            number: 12.25,
        };
        let name = PonkoStringArg {
            name: c"name".as_ptr(),
            kind: PonkoStringArgKind::PONKO_STRING_ARG_TEXT,
            text: c"Ada".as_ptr(),
            integer: 0,
            number: 0.0,
        };
        let sample = |seen: &mut Vec<String>| {
            seen.push(read(|b, c, l| ponko_ctx_strings_language(ctx, b, c, l)));
            seen.push(format(c"play", &[]));
            seen.push(format(c"coins", &[int(c"count", 1)]));
            seen.push(format(c"coins", &[int(c"count", 3)]));
            seen.push(format(c"time", &[seconds]));
            seen.push(format(c"hello", &[name]));
        };
        sample(&mut seen.en);

        // A label that holds a key.
        let mut root = 0;
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, null(), &mut root),
            PONKO_OK
        );
        let mut label = 0;
        assert_eq!(
            ponko_ctx_ui_add_label(ctx, PONKO_UI_SCREEN, root, c"".as_ptr(), null(), &mut label),
            PONKO_OK
        );
        let count = [int(c"count", 2)];
        assert_eq!(
            ponko_ctx_ui_set_text_key(ctx, PONKO_UI_SCREEN, label, c"coins".as_ptr(), count.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_ui_localize(ctx, PONKO_UI_SCREEN), PONKO_OK);
        let label_text = || read(|b, c, l| ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, label, b, c, l));
        seen.label_en = label_text();

        // A font for Arabic.
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/NotoSansArabic-Subset.ttf"
        ))
        .unwrap();
        let source = ponko_memory_source_new(c"fonts".as_ptr());
        assert_eq!(
            ponko_memory_source_insert(source, c"ar.ttf".as_ptr(), bytes.as_ptr(), bytes.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        let mut font = PonkoFont::default();
        assert_eq!(ponko_ctx_try_load_font(ctx, c"ar.ttf".as_ptr(), &mut font), PONKO_OK);
        assert_eq!(ponko_ctx_strings_set_font(ctx, c"ar".as_ptr(), font), PONKO_OK);
        assert_eq!(
            ponko_ctx_strings_set_font(ctx, c"ar".as_ptr(), PonkoFont { id: 999 }),
            PONKO_INVALID_ARGUMENT
        );

        assert_eq!(
            ponko_ctx_strings_set_language(ctx, c"xx".as_ptr(), &mut known),
            PONKO_OK
        );
        seen.unknown_kept = !known;
        assert_eq!(
            ponko_ctx_strings_set_language(ctx, c"ar".as_ptr(), &mut known),
            PONKO_OK
        );
        assert!(known);
        assert_eq!(ponko_ctx_strings_is_rtl(ctx, &mut seen.rtl), PONKO_OK);
        sample(&mut seen.ar);
        let mut current = PonkoFont::default();
        assert_eq!(ponko_ctx_strings_font(ctx, &mut seen.font_has, &mut current), PONKO_OK);
        seen.font_matches = current.id == font.id;
        assert_eq!(ponko_ctx_ui_localize(ctx, PONKO_UI_SCREEN), PONKO_OK);
        seen.label_ar = label_text();

        // Plain text lets go of the key.
        assert_eq!(
            ponko_ctx_ui_set_text(ctx, PONKO_UI_SCREEN, label, c"Fixed".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_strings_set_language(ctx, c"en".as_ptr(), &mut known),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_ui_localize(ctx, PONKO_UI_SCREEN), PONKO_OK);
        seen.label_plain = label_text();

        seen.report = read(|b, c, l| ponko_ctx_strings_missing_report(ctx, b, c, l));
    }
}

#[test]
fn string_tables_languages_plurals_and_keyed_labels_work_from_c() {
    let mut seen = Seen::default();
    run(Some(system), (&raw mut seen).cast());
    assert_eq!(seen.before_tables, Some(PONKO_NOT_FOUND), "no tables yet");
    assert_eq!(seen.load_missing, Some(PONKO_NOT_FOUND));
    assert_eq!(
        seen.languages,
        [
            ("en".to_string(), "English".to_string()),
            ("ar".to_string(), "العربية".to_string())
        ]
    );
    assert_eq!(
        seen.en,
        ["en", "Play", "1 coin", "3 coins", "Time 12.2 s", "Hello, Ada"]
    );
    assert_eq!(seen.ar, ["ar", "العب", "عملة", "٣ عملات", "الوقت ١٢.٢ ث", "Hello, Ada"]);
    assert!(seen.rtl);
    assert!(seen.unknown_kept);
    assert!(seen.font_has && seen.font_matches);
    assert_eq!(seen.label_en, "2 coins");
    assert_eq!(seen.label_ar, "عملتان");
    assert_eq!(seen.label_plain, "Fixed");
    assert_eq!(seen.report, "ar: \"hello\" is missing\n");
}

#[test]
fn plural_categories_from_c() {
    let category = |language: &CStr, n: f64| {
        let mut c = PonkoPluralCategory::PONKO_PLURAL_OTHER;
        assert_eq!(unsafe { ponko_plural_category(language.as_ptr(), n, &mut c) }, PONKO_OK);
        c
    };
    assert_eq!(category(c"en", 1.0), PonkoPluralCategory::PONKO_PLURAL_ONE);
    assert_eq!(category(c"ru", 3.0), PonkoPluralCategory::PONKO_PLURAL_FEW);
    assert_eq!(category(c"ar", 0.0), PonkoPluralCategory::PONKO_PLURAL_ZERO);
    assert_eq!(category(c"ja", 1.0), PonkoPluralCategory::PONKO_PLURAL_OTHER);
    assert_eq!(
        unsafe { ponko_plural_category(null(), 1.0, null_mut()) },
        PONKO_INVALID_ARGUMENT
    );
}
