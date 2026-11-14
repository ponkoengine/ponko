### Added
- Localization, `ponko::locale`: string tables, one JSON file per language,
  loaded with `Assets::load_strings` into the `Strings` resource and
  reloaded with hot reload. `Strings::get` and `format` look keys up in the
  current language (then the reference language, the first table loaded;
  a key no table has shows as itself), with `{name}` and `{name:2}`
  placeholders filled from `Arg`s; plural forms by the Unicode CLDR's
  categories (`PluralCategory`, `plural_category`, `plural_categories`)
  for Arabic, Hebrew, the Slavic and Baltic languages, Romanian, French,
  Portuguese, Hindi, the languages without plurals and English's rule for
  the rest; numbers with each table's separators and digits
  (`Strings::number`); `set_language`, `language`, `languages`,
  `is_rtl`, `revision`, and a font per language (`set_font`, `font`, or a
  table's `font` file, which the engine loads with the built-in font
  behind it). `Strings::missing` and `missing_report` list the keys, plural
  forms and arguments a language lacks, for a test; `Strings::PSEUDO`
  (`"en-XA"`) is a pseudo-locale with accented, longer text for testing
  layouts.
- Retained UI text from the string tables: `Ui::set_text_key` (with
  arguments), `UiBuilder::text_key` and `Ui::text_key`. Keyed nodes show
  the current language's text and font, and follow when the language or a
  table changes from the next frame on; `Ui::localize` does it for a UI you
  drive yourself. `Ui::set_text` replaces a key with plain text.
- Coin Dash ships in English and Arabic: its text comes from
  `assets/coin_dash/lang`, its settings have a language button and save
  the choice, and the Arabic font is a subset of Noto Sans Arabic (OFL).
  Arabic is offered when the game is built with `text-shaping`, which its
  packages and web build now enable. `tests/coin_dash_languages.rs` checks
  that the tables are complete and that the pseudo-locale still fits the
  screen.
- C and C++: `ponko_ctx_load_strings`, `ponko_ctx_strings_insert_json`,
  `ponko_ctx_strings_set_language`, `ponko_ctx_strings_language`,
  `ponko_ctx_strings_language_count`, `ponko_ctx_strings_language_at`,
  `ponko_ctx_strings_is_rtl`, `ponko_ctx_strings_set_font`,
  `ponko_ctx_strings_font`, `ponko_ctx_strings_format` (`PonkoStringArg`,
  `PonkoStringArgKind`), `ponko_ctx_strings_missing_report`,
  `ponko_plural_category` (`PonkoPluralCategory`),
  `ponko_ctx_ui_set_text_key` and `ponko_ctx_ui_localize`; in C++
  `ctx::load_strings`, `insert_strings_json`, `set_language`, `language`,
  `languages`, `language_is_rtl`, `set_language_font`, `language_font`,
  `format` and `missing_strings`, `ponko::arg`, `ponko::plural`,
  `ponko::plural_category`, and `ui::tree::set_text_key` and `localize`.
- The guide's chapter "Localization".
