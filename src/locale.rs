//! Localization: string tables, arguments, plural rules and switching the
//! language while the game runs.
//!
//! **What lives here.** [`Strings`], a resource holding one [`StringTable`]
//! per language: keyed lookup ([`Strings::get`]), arguments and plural forms
//! ([`Strings::format`]), the current language and a font per language, a
//! report of the keys a language lacks ([`Strings::missing`]) and a
//! pseudo-locale for testing layouts ([`Strings::PSEUDO`]).
//!
//! **When to call it.** Load the tables in setup with
//! [`Assets::load_strings`](crate::assets::Assets::load_strings): with hot
//! reload on, an edited table is applied again while the game runs. Change
//! the language from any system with [`Strings::set_language`]. Retained UI
//! nodes that hold a key ([`Ui::set_text_key`](crate::ui::Ui::set_text_key),
//! [`UiBuilder::text_key`](crate::ui::UiBuilder::text_key)) show the new
//! language's text and font from the next frame on; text drawn with
//! `draw_text` reads [`Strings::get`] when it is drawn, so it follows by itself.
//!
//! **What it needs.** Nothing: tables are plain data, so tests check them
//! headless.
//!
//! A table is a JSON file:
//!
//! ```json
//! {
//!   "language": "en",
//!   "name": "English",
//!   "strings": {
//!     "menu.play": "Play",
//!     "hud.time": "Time {seconds:1} s",
//!     "hud.coins": { "one": "{count} coin", "other": "{count} coins" }
//!   }
//! }
//! ```
//!
//! - `language` (required) is the language's code (`"en"`, `"pt-BR"`,
//!   `"ar"`); `name` is what a language menu shows (default: the code).
//! - `strings` maps keys to text. A value that is an object holds plural
//!   forms by [`PluralCategory`] name (`zero`, `one`, `two`, `few`, `many`,
//!   `other`); the `count` argument picks one with the language's rules.
//! - `{name}` in a text is replaced by the argument `name`; `{name:2}` shows a
//!   number with two decimals; `{{` and `}}` are literal braces.
//! - Optional: `direction` (`"rtl"` or `"ltr"`, default from the language
//!   code), `plural` (the code whose plural rules apply, default the
//!   language's own), `decimal` and `group` (the decimal and thousands
//!   separators, default `"."` and none), `digits` (ten characters for 0 to
//!   9, for example `"٠١٢٣٤٥٦٧٨٩"`) and `font` (a font file in `assets/`,
//!   loaded by the engine and used for this language's UI text; characters
//!   it lacks come from the built-in font).
//!
//! ```
//! use ponko::locale::{Arg, StringTable, Strings};
//!
//! let mut strings = Strings::default();
//! strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {
//!     "coins": {"one": "{count} coin", "other": "{count} coins"}}}"#).unwrap());
//! strings.insert(StringTable::from_json(r#"{"language": "fr", "strings": {
//!     "coins": {"one": "{count} pièce", "other": "{count} pièces"}}}"#).unwrap());
//! assert_eq!(strings.format("coins", &[("count", Arg::from(1))]), "1 coin");
//! strings.set_language("fr");
//! assert_eq!(strings.format("coins", &[("count", Arg::from(0))]), "0 pièce");
//! ```

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, ErrorKind};
use crate::text::Font;

/// The form of a word that goes with a number, by the Unicode CLDR's plural
/// categories: English has `One` ("1 coin") and `Other` ("2 coins"),
/// Arabic all six, Japanese only `Other`.
///
/// ```
/// use ponko::locale::{PluralCategory, plural_category};
/// assert_eq!(plural_category("en", 1.0), PluralCategory::One);
/// assert_eq!(plural_category("ru", 3.0), PluralCategory::Few);
/// assert_eq!(plural_category("ar", 11.0), PluralCategory::Many);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PluralCategory {
    /// Arabic's 0.
    Zero,
    /// The singular: English 1, French 0 and 1, Russian 21.
    One,
    /// The dual: Arabic and Hebrew 2.
    Two,
    /// Arabic 3–10, Russian and Polish 2–4, Czech 2–4.
    Few,
    /// Arabic 11–99, Russian and Polish 5–20.
    Many,
    /// Everything else; every language has it.
    Other,
}

impl PluralCategory {
    /// The category's name in a table file: `zero`, `one`, `two`, `few`,
    /// `many` or `other`.
    ///
    /// ```
    /// use ponko::locale::PluralCategory;
    /// assert_eq!(PluralCategory::Few.name(), "few");
    /// ```
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }

    /// The category named `name` in a table file.
    ///
    /// ```
    /// use ponko::locale::PluralCategory;
    /// assert_eq!(PluralCategory::from_name("many"), Some(PluralCategory::Many));
    /// assert_eq!(PluralCategory::from_name("lots"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "zero" => PluralCategory::Zero,
            "one" => PluralCategory::One,
            "two" => PluralCategory::Two,
            "few" => PluralCategory::Few,
            "many" => PluralCategory::Many,
            "other" => PluralCategory::Other,
            _ => return None,
        })
    }
}

/// The plural rules a language code follows (its part before `-` or `_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rules {
    /// Only `other`: Chinese, Japanese, Korean, Thai, Vietnamese, Indonesian.
    OtherOnly,
    /// `one` for 1 (English, German, Spanish, Italian, ...). Also the
    /// rules of a language not listed.
    OneIsOne,
    /// `one` for 0 and 1: French, Portuguese (Brazil), Hindi, Persian.
    ZeroAndOne,
    /// Russian, Ukrainian, Belarusian.
    EastSlavic,
    /// Croatian, Serbian, Bosnian.
    SouthSlavic,
    /// Polish.
    Polish,
    /// Czech, Slovak.
    CzechSlovak,
    /// Arabic.
    Arabic,
    /// Hebrew.
    Hebrew,
    /// Romanian, Moldavian.
    Romanian,
    /// Lithuanian.
    Lithuanian,
    /// Latvian.
    Latvian,
}

impl Rules {
    fn of(language: &str) -> Rules {
        let base = language.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        match base.as_str() {
            "ja" | "zh" | "ko" | "th" | "vi" | "id" | "ms" | "lo" | "my" | "km" | "yue" => Rules::OtherOnly,
            "fr" | "hi" | "fa" | "bn" | "gu" | "kn" | "am" | "zu" => Rules::ZeroAndOne,
            "pt" if !language.eq_ignore_ascii_case("pt-PT") => Rules::ZeroAndOne,
            "ru" | "uk" | "be" => Rules::EastSlavic,
            "hr" | "sr" | "bs" | "sh" => Rules::SouthSlavic,
            "pl" => Rules::Polish,
            "cs" | "sk" => Rules::CzechSlovak,
            "ar" => Rules::Arabic,
            "he" | "iw" => Rules::Hebrew,
            "ro" | "mo" => Rules::Romanian,
            "lt" => Rules::Lithuanian,
            "lv" => Rules::Latvian,
            _ => Rules::OneIsOne,
        }
    }

    fn categories(self) -> &'static [PluralCategory] {
        use PluralCategory::*;
        match self {
            Rules::OtherOnly => &[Other],
            Rules::OneIsOne | Rules::ZeroAndOne => &[One, Other],
            Rules::EastSlavic | Rules::Polish | Rules::CzechSlovak | Rules::Lithuanian => &[One, Few, Many, Other],
            Rules::SouthSlavic | Rules::Romanian => &[One, Few, Other],
            Rules::Arabic => &[Zero, One, Two, Few, Many, Other],
            Rules::Hebrew => &[One, Two, Other],
            Rules::Latvian => &[Zero, One, Other],
        }
    }

    fn category(self, count: f64) -> PluralCategory {
        use PluralCategory::*;
        let n = count.abs();
        // CLDR's operands: i is the integer part, `whole` says there are no
        // visible decimals (v = 0).
        let whole = n.fract() == 0.0;
        let i = n.trunc() as u64;
        let (i10, i100) = (i % 10, i % 100);
        match self {
            Rules::OtherOnly => Other,
            Rules::OneIsOne => {
                if i == 1 && whole {
                    One
                } else {
                    Other
                }
            }
            Rules::ZeroAndOne => {
                if i <= 1 {
                    One
                } else {
                    Other
                }
            }
            Rules::EastSlavic | Rules::SouthSlavic | Rules::Polish if !whole => Other,
            Rules::EastSlavic | Rules::SouthSlavic => {
                if i10 == 1 && i100 != 11 {
                    One
                } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                    Few
                } else if self == Rules::SouthSlavic {
                    Other
                } else {
                    Many
                }
            }
            Rules::Polish => {
                if i == 1 {
                    One
                } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                    Few
                } else {
                    Many
                }
            }
            Rules::CzechSlovak => {
                if !whole {
                    Many
                } else if i == 1 {
                    One
                } else if (2..=4).contains(&i) {
                    Few
                } else {
                    Other
                }
            }
            Rules::Arabic => {
                if !whole {
                    Other
                } else if i == 0 {
                    Zero
                } else if i == 1 {
                    One
                } else if i == 2 {
                    Two
                } else if (3..=10).contains(&i100) {
                    Few
                } else if (11..=99).contains(&i100) {
                    Many
                } else {
                    Other
                }
            }
            Rules::Hebrew => {
                if (whole && i == 1) || (!whole && i == 0) {
                    One
                } else if whole && i == 2 {
                    Two
                } else {
                    Other
                }
            }
            Rules::Romanian => {
                if whole && i == 1 {
                    One
                } else if !whole || i == 0 || (2..=19).contains(&i100) {
                    Few
                } else {
                    Other
                }
            }
            Rules::Lithuanian => {
                if !whole {
                    Many
                } else if i10 == 1 && !(11..=19).contains(&i100) {
                    One
                } else if (2..=9).contains(&i10) && !(11..=19).contains(&i100) {
                    Few
                } else {
                    Other
                }
            }
            Rules::Latvian => {
                if whole && (i10 == 0 || (11..=19).contains(&i100)) {
                    Zero
                } else if whole && i10 == 1 && i100 != 11 {
                    One
                } else {
                    Other
                }
            }
        }
    }
}

/// The plural form `count` takes in `language` (a code such as `"en"`,
/// `"pt-BR"` or `"ar"`), by the Unicode CLDR's cardinal rules.
///
/// Built in: Arabic, Hebrew, Russian, Ukrainian, Belarusian, Polish, Czech,
/// Slovak, Croatian, Serbian, Bosnian, Romanian, Lithuanian, Latvian, French,
/// Portuguese, Hindi, Persian, Bengali, the languages with no plural forms
/// (Chinese, Japanese, Korean, Thai, Vietnamese, Indonesian, Malay), and
/// English's rule (1 is `One`) for every other code. A number with a
/// fraction counts as having visible decimals (`1.5`; `1.0` is a whole 1).
///
/// ```
/// use ponko::locale::{PluralCategory, plural_category};
/// assert_eq!(plural_category("fr", 0.0), PluralCategory::One);
/// assert_eq!(plural_category("pl", 22.0), PluralCategory::Few);
/// assert_eq!(plural_category("pl", 25.0), PluralCategory::Many);
/// assert_eq!(plural_category("ja", 1.0), PluralCategory::Other);
/// ```
#[must_use]
pub fn plural_category(language: &str, count: f64) -> PluralCategory {
    Rules::of(language).category(count)
}

/// The plural categories `language` uses, `Other` last: what a table for it
/// must give a plural key (see [`Strings::missing`]).
///
/// ```
/// use ponko::locale::{PluralCategory, plural_categories};
/// assert_eq!(plural_categories("en"), &[PluralCategory::One, PluralCategory::Other]);
/// assert_eq!(plural_categories("ar").len(), 6);
/// ```
#[must_use]
pub fn plural_categories(language: &str) -> &'static [PluralCategory] {
    Rules::of(language).categories()
}

/// An argument of [`Strings::format`]: text, a whole number or a number with
/// decimals. Numbers are written with the language's separators and digits.
///
/// ```
/// use ponko::locale::Arg;
/// assert_eq!(Arg::from(3), Arg::Int(3));
/// assert_eq!(Arg::from("Ada"), Arg::Text("Ada".into()));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// Inserted as it is.
    Text(String),
    /// A whole number.
    Int(i64),
    /// A number: as many decimals as the placeholder asks for (`{time:1}`),
    /// or as few as show it exactly.
    Number(f64),
}

impl Arg {
    /// The number this argument holds, if any: the `count` argument picks
    /// the plural form with it.
    fn number(&self) -> Option<f64> {
        match self {
            Arg::Text(_) => None,
            Arg::Int(n) => Some(*n as f64),
            Arg::Number(n) => Some(*n),
        }
    }
}

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Arg::Text(text.to_string())
    }
}
impl From<String> for Arg {
    fn from(text: String) -> Self {
        Arg::Text(text)
    }
}
impl From<&String> for Arg {
    fn from(text: &String) -> Self {
        Arg::Text(text.clone())
    }
}
macro_rules! int_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(n: $t) -> Self {
                Arg::Int(n as i64)
            }
        }
    )*};
}
int_arg!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);
impl From<f32> for Arg {
    fn from(n: f32) -> Self {
        Arg::Number(f64::from(n))
    }
}
impl From<f64> for Arg {
    fn from(n: f64) -> Self {
        Arg::Number(n)
    }
}

/// One key's text: plain, or one text per plural form.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Text(String),
    Plural(Vec<(PluralCategory, String)>),
}

impl Entry {
    /// The text for `form`, else `other`, else the first.
    fn form(&self, form: PluralCategory) -> &str {
        match self {
            Entry::Text(t) => t,
            Entry::Plural(forms) => forms
                .iter()
                .find(|(c, _)| *c == form)
                .or_else(|| forms.iter().find(|(c, _)| *c == PluralCategory::Other))
                .or(forms.first())
                .map_or("", |(_, t)| t),
        }
    }

    fn texts(&self) -> impl Iterator<Item = &str> {
        let (one, many) = match self {
            Entry::Text(t) => (Some(t.as_str()), &[][..]),
            Entry::Plural(forms) => (None, forms.as_slice()),
        };
        one.into_iter().chain(many.iter().map(|(_, t)| t.as_str()))
    }
}

/// One language's strings, read from a table file (see the [module](self)
/// for the format).
///
/// ```
/// use ponko::locale::StringTable;
/// let table = StringTable::from_json(r#"{"language": "ar", "name": "العربية",
///     "strings": {"menu.play": "العب"}}"#).unwrap();
/// assert_eq!(table.language(), "ar");
/// assert!(table.is_rtl());
/// assert!(table.contains("menu.play"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StringTable {
    language: String,
    name: String,
    rtl: bool,
    plural: String,
    decimal: String,
    group: String,
    digits: Option<[char; 10]>,
    font: Option<String>,
    strings: HashMap<String, Entry>,
}

/// Languages written right to left, by code.
const RTL: &[&str] = &["ar", "he", "iw", "fa", "ur", "ps", "sd", "ug", "yi", "dv", "ckb"];

impl StringTable {
    /// An empty table for `language`.
    ///
    /// ```
    /// use ponko::locale::StringTable;
    /// let mut table = StringTable::new("de");
    /// table.set("menu.play", "Spielen");
    /// assert_eq!(table.len(), 1);
    /// ```
    #[must_use]
    pub fn new(language: &str) -> Self {
        let base = language.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        StringTable {
            language: language.to_string(),
            name: language.to_string(),
            rtl: RTL.contains(&base.as_str()),
            plural: language.to_string(),
            decimal: ".".to_string(),
            group: String::new(),
            digits: None,
            font: None,
            strings: HashMap::new(),
        }
    }

    /// Reads a table file's text.
    ///
    /// # Errors
    /// [`ErrorKind::Decode`] if it isn't JSON, has no `language`, or a value
    /// is neither text nor an object of plural forms named by
    /// [`PluralCategory`] (the message names the key).
    pub fn from_json(text: &str) -> Result<Self, Error> {
        use serde_json::Value;
        let bad = |message: String| Error::new(ErrorKind::Decode, message);
        let value: Value = serde_json::from_str(text)?;
        let Value::Object(top) = value else {
            return Err(bad("a string table is a JSON object".into()));
        };
        let text_field = |name: &str| -> Result<Option<String>, Error> {
            match top.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(bad(format!("\"{name}\" must be text"))),
            }
        };
        let language = text_field("language")?
            .filter(|l| !l.is_empty())
            .ok_or_else(|| bad("a string table needs \"language\", its language code".into()))?;
        let mut table = StringTable::new(&language);
        if let Some(name) = text_field("name")? {
            table.name = name;
        }
        match text_field("direction")?.as_deref() {
            None => {}
            Some("rtl") => table.rtl = true,
            Some("ltr") => table.rtl = false,
            Some(other) => return Err(bad(format!("\"direction\" is \"rtl\" or \"ltr\", not \"{other}\""))),
        }
        if let Some(plural) = text_field("plural")? {
            table.plural = plural;
        }
        if let Some(decimal) = text_field("decimal")? {
            table.decimal = decimal;
        }
        if let Some(group) = text_field("group")? {
            table.group = group;
        }
        if let Some(digits) = text_field("digits")? {
            let chars: Vec<char> = digits.chars().collect();
            let digits: [char; 10] = chars
                .try_into()
                .map_err(|_| bad("\"digits\" holds ten characters, for 0 to 9".into()))?;
            table.digits = Some(digits);
        }
        table.font = text_field("font")?;
        match top.get("strings") {
            None | Some(Value::Null) => {}
            Some(Value::Object(strings)) => {
                for (key, value) in strings {
                    let entry = match value {
                        Value::String(s) => Entry::Text(s.clone()),
                        Value::Object(forms) => {
                            let mut list = Vec::with_capacity(forms.len());
                            for (form, text) in forms {
                                let category = PluralCategory::from_name(form).ok_or_else(|| {
                                    bad(format!(
                                        "\"{key}\": \"{form}\" is not a plural form (zero, one, two, few, many, other)"
                                    ))
                                })?;
                                let Value::String(text) = text else {
                                    return Err(bad(format!("\"{key}\".\"{form}\" must be text")));
                                };
                                list.push((category, text.clone()));
                            }
                            list.sort_by_key(|(c, _)| *c);
                            Entry::Plural(list)
                        }
                        _ => return Err(bad(format!("\"{key}\" must be text or an object of plural forms"))),
                    };
                    table.strings.insert(key.clone(), entry);
                }
            }
            Some(_) => return Err(bad("\"strings\" must be an object of keys".into())),
        }
        Ok(table)
    }

    /// The language's code, as the file gives it.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The language's name for a menu (the file's `name`, or the code).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the language is written right to left.
    #[must_use]
    pub fn is_rtl(&self) -> bool {
        self.rtl
    }

    /// The font file the table names (`font`), if any.
    #[must_use]
    pub fn font_path(&self) -> Option<&str> {
        self.font.as_deref()
    }

    /// Whether the table has text for `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.strings.contains_key(key)
    }

    /// How many keys the table has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the table has no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The table's keys, sorted.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.strings.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    /// Sets `key`'s text (one text for every plural form).
    pub fn set(&mut self, key: &str, text: &str) {
        self.strings.insert(key.to_string(), Entry::Text(text.to_string()));
    }

    /// Sets `key`'s text for one plural `form`; a plain text the key had
    /// becomes its `other` form.
    ///
    /// ```
    /// use ponko::locale::{Arg, PluralCategory, StringTable, Strings};
    /// let mut table = StringTable::new("en");
    /// table.set_plural("lives", PluralCategory::One, "{count} life");
    /// table.set_plural("lives", PluralCategory::Other, "{count} lives");
    /// let mut strings = Strings::default();
    /// strings.insert(table);
    /// assert_eq!(strings.format("lives", &[("count", Arg::from(3))]), "3 lives");
    /// ```
    pub fn set_plural(&mut self, key: &str, form: PluralCategory, text: &str) {
        let entry = self
            .strings
            .entry(key.to_string())
            .or_insert_with(|| Entry::Plural(Vec::new()));
        if let Entry::Text(old) = entry {
            *entry = Entry::Plural(vec![(PluralCategory::Other, std::mem::take(old))]);
        }
        let Entry::Plural(forms) = entry else { unreachable!() };
        forms.retain(|(c, _)| *c != form);
        forms.push((form, text.to_string()));
        forms.sort_by_key(|(c, _)| *c);
    }

    /// Writes `value` with the table's separators and digits: `decimals`
    /// digits after the point, or `None` for as few as show it exactly.
    fn write_number(&self, out: &mut String, value: f64, decimals: Option<usize>) {
        let mut plain = String::new();
        match decimals {
            Some(d) => {
                let _ = write!(plain, "{value:.d$}");
            }
            None => {
                let _ = write!(plain, "{value}");
            }
        }
        self.localize_digits(out, &plain);
    }

    /// Copies `plain` (`-1234.5` as Rust writes it) to `out` with the
    /// table's group and decimal separators and digits.
    fn localize_digits(&self, out: &mut String, plain: &str) {
        let (sign, rest) = match plain.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", plain),
        };
        let (int, frac) = match rest.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (rest, None),
        };
        out.push_str(sign);
        let digit = |c: char| match (self.digits, c.to_digit(10)) {
            (Some(d), Some(n)) => d[n as usize],
            _ => c,
        };
        let n = int.len();
        for (at, c) in int.chars().enumerate() {
            if at > 0 && !self.group.is_empty() && (n - at) % 3 == 0 && int.bytes().all(|b| b.is_ascii_digit()) {
                out.push_str(&self.group);
            }
            out.push(digit(c));
        }
        if let Some(frac) = frac {
            out.push_str(&self.decimal);
            out.extend(frac.chars().map(digit));
        }
    }
}

/// A gap [`Strings::missing`] found in a language's table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Missing {
    /// The language whose table has the gap.
    pub language: String,
    /// The key.
    pub key: String,
    /// What is missing.
    pub what: MissingKind,
}

/// What a [`Missing`] lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MissingKind {
    /// The key: the reference language has it, this language doesn't (the
    /// reference language's text is shown instead).
    Key,
    /// A plural form the language's rules use (`other` is shown instead).
    PluralForm(PluralCategory),
    /// An argument the reference language's text uses and this text doesn't,
    /// such as a `{count}` a translation dropped.
    Argument(String),
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.what {
            MissingKind::Key => write!(f, "{}: \"{}\" is missing", self.language, self.key),
            MissingKind::PluralForm(c) => write!(
                f,
                "{}: \"{}\" has no \"{}\" plural form",
                self.language,
                self.key,
                c.name()
            ),
            MissingKind::Argument(a) => write!(f, "{}: \"{}\" doesn't use {{{a}}}", self.language, self.key),
        }
    }
}

/// Unique revisions across every [`Strings`], so a UI that followed one
/// set of tables notices another.
static REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

/// The game's string tables and its current language: a resource,
/// `ctx.world.resource::<Strings>()` (see the [module](self)).
///
/// The first table inserted is the **reference** language: its keys are
/// the ones every other language should have ([`missing`](Self::missing)),
/// and its text is shown for a key the current language lacks. A key no
/// table has is shown as the key itself, so it is easy to spot.
///
/// ```
/// use ponko::locale::{StringTable, Strings};
/// let mut strings = Strings::default();
/// strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"play": "Play", "quit": "Quit"}}"#).unwrap());
/// strings.insert(StringTable::from_json(r#"{"language": "de", "strings": {"play": "Spielen"}}"#).unwrap());
/// assert!(strings.set_language("de"));
/// assert_eq!(strings.get("play"), "Spielen");
/// assert_eq!(strings.get("quit"), "Quit"); // from the reference language
/// assert_eq!(strings.get("nope"), "nope");
/// assert_eq!(strings.missing("de").len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct Strings {
    tables: Vec<StringTable>,
    /// Index of the current language's table, or `None` for the
    /// pseudo-locale or no table.
    current: Option<usize>,
    pseudo: bool,
    /// Fonts by language, set with `set_font` or loaded from a table's `font`.
    fonts: Vec<(String, Font)>,
    /// Table font files the engine tried to load already (`font` paths).
    pub(crate) fonts_tried: Vec<(String, String)>,
    revision: u64,
}

impl Default for Strings {
    fn default() -> Self {
        Strings {
            tables: Vec::new(),
            current: None,
            pseudo: false,
            fonts: Vec::new(),
            fonts_tried: Vec::new(),
            revision: next_revision(),
        }
    }
}

impl Strings {
    /// The pseudo-locale's code: the reference language with its letters
    /// accented and every text a third longer, in brackets
    /// (`"Play"` becomes `"[Ƥļàý···]"`). Switch to it to see, without a
    /// translation, which text isn't localized (it stays plain) and where
    /// longer translations will overflow or be cut (the closing bracket is
    /// missing).
    ///
    /// ```
    /// use ponko::locale::{StringTable, Strings};
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"play": "Play {n}"}}"#).unwrap());
    /// strings.set_language(Strings::PSEUDO);
    /// let text = strings.get("play");
    /// assert!(text.starts_with('[') && text.ends_with(']'), "{text}");
    /// assert!(text.contains("{n}"), "placeholders stay: {text}");
    /// assert!(text.chars().count() > "[Play {n}]".len());
    /// ```
    pub const PSEUDO: &'static str = "en-XA";

    /// Adds a language's table, or replaces the table of the same language
    /// (a reloaded file). The first table is the reference language and
    /// becomes the current one.
    pub fn insert(&mut self, table: StringTable) {
        match self.tables.iter().position(|t| t.language == table.language) {
            Some(at) => {
                // A different font file is loaded again.
                if self.tables[at].font != table.font {
                    self.fonts_tried.retain(|(l, _)| *l != table.language);
                }
                self.tables[at] = table;
            }
            None => {
                self.tables.push(table);
                if self.tables.len() == 1 && !self.pseudo {
                    self.current = Some(0);
                }
            }
        }
        self.revision = next_revision();
    }

    /// The languages with a table, reference first: their codes, for a
    /// language menu ([`table`](Self::table) has their names). The
    /// pseudo-locale isn't listed.
    #[must_use]
    pub fn languages(&self) -> Vec<&str> {
        self.tables.iter().map(|t| t.language.as_str()).collect()
    }

    /// The table of `language`.
    #[must_use]
    pub fn table(&self, language: &str) -> Option<&StringTable> {
        self.tables.iter().find(|t| t.language == language)
    }

    /// The reference language's code (the first table's), or `""` with no
    /// tables.
    #[must_use]
    pub fn reference(&self) -> &str {
        self.tables.first().map_or("", |t| t.language.as_str())
    }

    /// The current language's code: [`PSEUDO`](Self::PSEUDO) for the
    /// pseudo-locale, `""` with no tables.
    #[must_use]
    pub fn language(&self) -> &str {
        if self.pseudo {
            return Self::PSEUDO;
        }
        self.current.map_or("", |i| self.tables[i].language.as_str())
    }

    /// Switches to `language`: a code with a table, or
    /// [`PSEUDO`](Self::PSEUDO). Returns whether it could; an unknown code
    /// keeps the current language. UI nodes that hold a key follow on the
    /// next frame.
    pub fn set_language(&mut self, language: &str) -> bool {
        if language == Self::PSEUDO {
            if !self.pseudo {
                self.pseudo = true;
                self.revision = next_revision();
            }
            return true;
        }
        let Some(at) = self.tables.iter().position(|t| t.language == language) else {
            return false;
        };
        if self.pseudo || self.current != Some(at) {
            self.pseudo = false;
            self.current = Some(at);
            self.revision = next_revision();
        }
        true
    }

    /// Changes whenever the text could have: a table inserted or reloaded,
    /// the language or a font changed. Code that caches localized text
    /// redoes it when this differs from the value it saw.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the current language is written right to left.
    #[must_use]
    pub fn is_rtl(&self) -> bool {
        !self.pseudo && self.current.is_some_and(|i| self.tables[i].rtl)
    }

    /// Sets the font `language`'s UI text is drawn with (over a table's
    /// `font` file).
    pub fn set_font(&mut self, language: &str, font: Font) {
        self.fonts.retain(|(l, _)| l != language);
        self.fonts.push((language.to_string(), font));
        self.revision = next_revision();
    }

    /// The current language's font, if it has one (the pseudo-locale uses
    /// the reference language's). Draw text with it:
    /// `TextParams::default().with_font(strings.font().unwrap_or(Font::DEFAULT))`.
    #[must_use]
    pub fn font(&self) -> Option<Font> {
        let language = if self.pseudo { self.reference() } else { self.language() };
        self.fonts.iter().find(|(l, _)| l == language).map(|(_, f)| *f)
    }

    /// Table fonts to load: (language, file) pairs not tried yet.
    pub(crate) fn fonts_to_load(&self) -> Vec<(String, String)> {
        self.tables
            .iter()
            .filter_map(|t| Some((t.language.clone(), t.font.clone()?)))
            .filter(|(l, _)| !self.fonts_tried.iter().any(|(tried, _)| tried == l))
            .collect()
    }

    /// The table to read `key` from, and whether the pseudo-locale applies.
    fn lookup(&self, key: &str) -> Option<(&StringTable, &Entry)> {
        if !self.pseudo
            && let Some(i) = self.current
            && let Some(entry) = self.tables[i].strings.get(key)
        {
            return Some((&self.tables[i], entry));
        }
        let reference = self.tables.first()?;
        reference.strings.get(key).map(|e| (reference, e))
    }

    /// The table whose separators, digits and plural rules apply now.
    fn number_table(&self) -> Option<&StringTable> {
        if self.pseudo {
            return self.tables.first();
        }
        self.current.map(|i| &self.tables[i])
    }

    /// `key`'s text in the current language, with no arguments (a plural
    /// key gives its `other` form, with its placeholders). Borrowed unless
    /// it is the pseudo-locale.
    ///
    /// ```
    /// use ponko::locale::{StringTable, Strings};
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"title": "Coin Dash"}}"#).unwrap());
    /// assert_eq!(strings.get("title"), "Coin Dash");
    /// ```
    #[must_use]
    pub fn get<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        let Some((_, entry)) = self.lookup(key) else {
            return Cow::Borrowed(key);
        };
        let text = entry.form(PluralCategory::Other);
        if self.pseudo {
            let mut out = String::new();
            pseudo(text, &mut out);
            Cow::Owned(out)
        } else {
            Cow::Borrowed(text)
        }
    }

    /// Whether the current language or the reference language has `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.lookup(key).is_some()
    }

    /// `key`'s text with `args` put in its placeholders. For a plural key,
    /// the argument `count` picks the form by the current language's rules.
    /// A placeholder with no argument stays as it is (`{name}`).
    ///
    /// ```
    /// use ponko::locale::{Arg, StringTable, Strings};
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "de", "decimal": ",", "group": ".",
    ///     "strings": {"best": "Bestzeit {time:1} s, {points} Punkte"}}"#).unwrap());
    /// let text = strings.format("best", &[("time", Arg::from(12.25)), ("points", Arg::from(12500))]);
    /// assert_eq!(text, "Bestzeit 12,2 s, 12.500 Punkte");
    /// ```
    #[must_use]
    pub fn format(&self, key: &str, args: &[(&str, Arg)]) -> String {
        let mut out = String::new();
        self.format_into(key, args, &mut out);
        out
    }

    /// [`format`](Self::format) into `out` (cleared first), so text
    /// redrawn every frame reuses one string.
    pub fn format_into(&self, key: &str, args: &[(&str, Arg)], out: &mut String) {
        out.clear();
        let Some((table, entry)) = self.lookup(key) else {
            out.push_str(key);
            return;
        };
        let numbers = self.number_table().unwrap_or(table);
        let form = match args
            .iter()
            .find(|(name, _)| *name == "count")
            .and_then(|(_, a)| a.number())
        {
            Some(count) => plural_category(&numbers.plural, count),
            None => PluralCategory::Other,
        };
        let template = entry.form(form);
        if self.pseudo {
            let mut accented = String::new();
            pseudo(template, &mut accented);
            fill(&accented, args, numbers, out);
        } else {
            fill(template, args, numbers, out);
        }
    }

    /// Writes `value` as the current language writes numbers: its decimal
    /// and group separators and digits, `decimals` digits after the point
    /// (`None`: as few as show it exactly).
    ///
    /// ```
    /// use ponko::locale::{StringTable, Strings};
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "fr", "decimal": ",", "group": " "}"#).unwrap());
    /// assert_eq!(strings.number(1234567.891, Some(2)), "1 234 567,89");
    /// ```
    #[must_use]
    pub fn number(&self, value: f64, decimals: Option<usize>) -> String {
        let mut out = String::new();
        match self.number_table() {
            Some(t) => t.write_number(&mut out, value, decimals),
            None => StringTable::new("en").write_number(&mut out, value, decimals),
        }
        out
    }

    /// What `language`'s table lacks compared with the reference language:
    /// keys, plural forms its rules use, and arguments the reference text
    /// uses. The reference language itself is checked for plural forms.
    /// Sorted by key; empty for an unknown language.
    #[must_use]
    pub fn missing(&self, language: &str) -> Vec<Missing> {
        let (Some(reference), Some(table)) = (self.tables.first(), self.table(language)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let gap = |key: &str, what| Missing {
            language: language.to_string(),
            key: key.to_string(),
            what,
        };
        let mut keys: Vec<&String> = reference.strings.keys().chain(table.strings.keys()).collect();
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            let Some(entry) = table.strings.get(key) else {
                out.push(gap(key, MissingKind::Key));
                continue;
            };
            if let Entry::Plural(forms) = entry {
                for &category in plural_categories(&table.plural) {
                    if !forms.iter().any(|(c, _)| *c == category) {
                        out.push(gap(key, MissingKind::PluralForm(category)));
                    }
                }
            }
            if let Some(original) = reference.strings.get(key) {
                let mut used = Vec::new();
                for text in entry.texts() {
                    placeholders(text, &mut used);
                }
                let mut wanted = Vec::new();
                for text in original.texts() {
                    placeholders(text, &mut wanted);
                }
                wanted.sort_unstable();
                wanted.dedup();
                for name in wanted {
                    // A form for exactly one thing may leave the count out ("a coin").
                    if !used.contains(&name) && !(name == "count" && matches!(entry, Entry::Plural(_))) {
                        out.push(gap(key, MissingKind::Argument(name.to_string())));
                    }
                }
            }
        }
        out
    }

    /// Every language's [`missing`](Self::missing) gaps, one per line, or
    /// `""` when every table is complete: for a test that keeps
    /// translations complete.
    ///
    /// ```
    /// use ponko::locale::{StringTable, Strings};
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"a": "A", "b": "B"}}"#).unwrap());
    /// strings.insert(StringTable::from_json(r#"{"language": "es", "strings": {"a": "A"}}"#).unwrap());
    /// assert_eq!(strings.missing_report(), "es: \"b\" is missing\n");
    /// ```
    #[must_use]
    pub fn missing_report(&self) -> String {
        let mut out = String::new();
        for table in &self.tables {
            for gap in self.missing(&table.language) {
                let _ = writeln!(out, "{gap}");
            }
        }
        out
    }
}

/// Collects the placeholder names in `text` (`{name}` and `{name:2}`).
fn placeholders<'a>(text: &'a str, out: &mut Vec<&'a str>) {
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(stripped) = after.strip_prefix('{') {
            rest = stripped;
            continue;
        }
        let Some(close) = after.find('}') else { break };
        let inner = &after[..close];
        out.push(inner.split(':').next().unwrap_or(inner));
        rest = &after[close + 1..];
    }
}

/// Writes `template` to `out` with its placeholders filled from `args`.
fn fill(template: &str, args: &[(&str, Arg)], numbers: &StringTable, out: &mut String) {
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let brace = rest.as_bytes()[at];
        let after = &rest[at + 1..];
        if after.as_bytes().first() == Some(&brace) {
            out.push(brace as char);
            rest = &after[1..];
            continue;
        }
        if brace == b'}' {
            out.push('}');
            rest = after;
            continue;
        }
        let Some(close) = after.find('}') else {
            out.push_str(&rest[at..]);
            return;
        };
        let inner = &after[..close];
        let (name, decimals) = match inner.split_once(':') {
            Some((name, d)) => (name, d.trim().parse::<usize>().ok()),
            None => (inner, None),
        };
        match args.iter().find(|(n, _)| *n == name) {
            Some((_, Arg::Text(t))) => out.push_str(t),
            Some((_, Arg::Int(n))) => match decimals {
                Some(_) => numbers.write_number(out, *n as f64, decimals),
                None => numbers.localize_digits(out, &n.to_string()),
            },
            Some((_, Arg::Number(n))) => numbers.write_number(out, *n, decimals),
            None => {
                out.push('{');
                out.push_str(inner);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
}

/// The pseudo-locale's form of `text`: letters accented, about a third
/// longer, in brackets; placeholders and `\n` kept.
fn pseudo(text: &str, out: &mut String) {
    const FROM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const TO: [char; 52] = [
        'à', 'ƀ', 'ç', 'ð', 'é', 'ƒ', 'ĝ', 'ĥ', 'î', 'ĵ', 'ķ', 'ļ', 'ɱ', 'ñ', 'ö', 'þ', 'ǫ', 'ŕ', 'š', 'ţ', 'û', 'ṽ',
        'ŵ', 'ẋ', 'ý', 'ž', 'Å', 'Ɓ', 'Ç', 'Ð', 'É', 'Ƒ', 'Ĝ', 'Ĥ', 'Î', 'Ĵ', 'Ķ', 'Ļ', 'Ṁ', 'Ñ', 'Ö', 'Ƥ', 'Ǫ', 'Ŕ',
        'Š', 'Ţ', 'Û', 'Ṽ', 'Ŵ', 'Ẋ', 'Ý', 'Ž',
    ];
    out.push('[');
    let mut letters = 0usize;
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && let Some(i) = FROM.find(c)
        {
            out.push(TO[i]);
            letters += 1;
        } else {
            out.push(c);
        }
    }
    // A third longer: translations into German or Finnish often are.
    for _ in 0..letters.div_ceil(3).max(1) {
        out.push('·');
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_escapes_and_unknown_arguments() {
        let t = StringTable::new("en");
        let mut out = String::new();
        fill("{{a}} {b} {c} }}", &[("b", Arg::from("B"))], &t, &mut out);
        assert_eq!(out, "{a} B {c} }");
        let mut names = Vec::new();
        placeholders("{{x}} {a} {b:2}", &mut names);
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn digits_and_groups() {
        let mut t = StringTable::new("ar");
        t.digits = Some(['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩']);
        t.decimal = "٫".into();
        t.group = "٬".into();
        let mut out = String::new();
        t.write_number(&mut out, -12345.5, Some(1));
        assert_eq!(out, "-١٢٬٣٤٥٫٥");
    }
}
