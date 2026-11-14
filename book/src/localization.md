# Localization

A game translated into other languages keeps its text in **string tables**,
one file per language, and asks for it by key. `ponko::locale` holds them in
a resource, [`Strings`](https://docs.rs/ponko/latest/ponko/locale/struct.Strings.html):
keyed lookup with arguments, plural forms by each language's rules, numbers
written the language's way, the current language and a font per language,
a report of what a translation lacks, and a pseudo-locale for testing
layouts. UI labels that hold a key follow the language by themselves.

The reference game [Coin Dash](packaging.md) ships in English and Arabic
this way: its tables are `assets/coin_dash/lang/en.json` and `ar.json`, and
its settings screen has a language button.

## A string table

```json
{
  "language": "en",
  "name": "English",
  "strings": {
    "menu.play": "Play",
    "hud.time": "Time {seconds:1} s",
    "hud.coins": { "one": "{count} coin", "other": "{count} coins" }
  }
}
```

- `language` is the language's code (`"en"`, `"pt-BR"`, `"ar"`); `name` is
  what a language menu shows.
- `strings` maps keys to text. `{name}` is replaced by the argument `name`,
  `{name:2}` shows a number with two decimals, `{{` and `}}` are braces.
- A value that is an object holds **plural forms**, named by the Unicode
  CLDR's categories: `zero`, `one`, `two`, `few`, `many`, `other`. The
  argument `count` picks one with the language's rules: English uses `one`
  and `other`, Russian and Polish `one`, `few`, `many` and `other`, Arabic
  all six, Japanese only `other`.
- Optional: `direction` (`"rtl"` or `"ltr"`; Arabic, Hebrew, Persian and
  Urdu codes are right to left already), `plural` (another code whose rules
  apply), `decimal` and `group` (the separators: `"."` and none unless set),
  `digits` (ten characters for 0 to 9, such as `"٠١٢٣٤٥٦٧٨٩"`) and `font`
  (a font file the engine loads for this language's UI text).

## Loading and asking

Load each table in setup. The first one is the **reference language**: its
text is shown for a key another language lacks, and it is the current
language until you switch. A key no table has is shown as the key itself,
so it stands out on screen.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
use ponko::locale::{Arg, Strings};
# fn setup(ctx: &mut Ctx) -> Result<(), Error> {
ctx.assets.load_strings(&mut ctx.world, "lang/en.json")?;
ctx.assets.load_strings(&mut ctx.world, "lang/fr.json")?;

let root = ctx.ui.root().id();
let play = ctx.ui.button(root, "").text_key("menu.play").id();
ctx.ui.push_screen(root, false);

let strings = ctx.world.resource_mut::<Strings>();
strings.set_language("fr");
let coins = strings.format("hud.coins", &[("count", Arg::from(0))]); // "0 pièce"
# let _ = (play, coins);
# Ok(())
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:string_tables}}
```

</div>
</div>

[`Strings::get`](https://docs.rs/ponko/latest/ponko/locale/struct.Strings.html#method.get)
gives a key's text without arguments (borrowed, so it costs nothing to draw
every frame), [`format`](https://docs.rs/ponko/latest/ponko/locale/struct.Strings.html#method.format)
fills its placeholders, and `format_into` does it into a string you keep, so
text redrawn every frame allocates nothing. `Arg::from` takes text, whole
numbers and numbers with decimals; numbers are written with the table's
separators and digits:

```rust
# use ponko::locale::{Arg, StringTable, Strings};
# let mut strings = Strings::default();
# strings.insert(StringTable::from_json(r#"{"language": "de", "decimal": ",", "group": ".",
#     "strings": {"best": "Bestzeit {time:1} s, {points} Punkte"}}"#).unwrap());
let text = strings.format("best", &[("time", Arg::from(12.25)), ("points", Arg::from(12500))]);
assert_eq!(text, "Bestzeit 12,2 s, 12.500 Punkte");
```

`ponko::locale::plural_category(language, count)` answers which form a
number takes, for code that builds text itself.

## Switching the language

[`Strings::set_language`](https://docs.rs/ponko/latest/ponko/locale/struct.Strings.html#method.set_language)
switches from any system and says whether the language has a table.
Retained UI nodes that hold a key (`UiBuilder::text_key`, or
`Ui::set_text_key` with arguments) show the new language's text from the
next frame on, and its font if it has one; `Ui::set_text` replaces a key
with plain text. Text you draw with `draw_text` follows because you ask for
it as you draw it. Code that caches localized text compares
`Strings::revision` with the value it saw. `Strings::languages` and each
table's `name` fill a language menu; `Strings::is_rtl` says whether to lay
your own text out from the right.

Right-to-left scripts and scripts whose letters join (Arabic, Hebrew,
Devanagari) need the `text-shaping` feature: without it each character is
drawn on its own, left to right. Coin Dash offers Arabic only when it is
built with the feature, and its packages and web build are. A language's
font comes from the table's `font` (characters it lacks come from the
built-in font, so key names in Latin letters still draw) or from
`Strings::set_font`; draw your own text with `strings.font()`.

With [hot reload](data-files.md) on, an edited table is applied while the
game runs and keyed labels change at once; a table that doesn't parse keeps
the old one and prints why.

## Keeping translations complete

[`Strings::missing`](https://docs.rs/ponko/latest/ponko/locale/struct.Strings.html#method.missing)
lists what a language's table lacks compared with the reference language:
keys, the plural forms its rules use, and arguments the reference text uses
that a translation dropped. `missing_report` lists every language's gaps,
one per line, so a test keeps the tables complete:

```rust
# use ponko::locale::{StringTable, Strings};
# let mut strings = Strings::default();
# strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"play": "Play"}}"#).unwrap());
# strings.insert(StringTable::from_json(r#"{"language": "fr", "strings": {"play": "Jouer"}}"#).unwrap());
let report = strings.missing_report();
assert!(report.is_empty(), "the string tables have gaps:\n{report}");
```

Coin Dash's `tests/coin_dash_languages.rs` does that, and also checks that
every key its source asks for is in the English table.

## The pseudo-locale

`strings.set_language(Strings::PSEUDO)` (`"en-XA"`) shows the reference
language with its letters accented and every text a third longer, in
brackets: `"Play"` becomes `"[Ƥļàý··]"`. Text that isn't localized stays
plain, and text that a longer translation would push out of its box loses
its closing bracket. Coin Dash's tests measure every screen's lines in the
pseudo-locale and check they still fit.

## From C and C++

`ponko_ctx_load_strings` and `ponko_ctx_strings_insert_json` add tables;
`ponko_ctx_strings_set_language`, `ponko_ctx_strings_language`,
`ponko_ctx_strings_language_count`, `ponko_ctx_strings_language_at`,
`ponko_ctx_strings_is_rtl`, `ponko_ctx_strings_set_font`,
`ponko_ctx_strings_font`, `ponko_ctx_strings_format` (with `PonkoStringArg`
arguments) and `ponko_ctx_strings_missing_report` do what the Rust calls
do; `ponko_plural_category` answers plural forms and
`ponko_ctx_ui_set_text_key` gives a node a key (`ponko_ctx_ui_localize`
fills keyed nodes in when you drive a UI by hand). In C++ they are
`ctx::load_strings`, `insert_strings_json`, `set_language`, `language`,
`languages`, `language_is_rtl`, `set_language_font`, `language_font`,
`format` (with `ponko::arg`) and `missing_strings`, `ponko::plural`, and
`ui::tree::set_text_key` and `localize`.
