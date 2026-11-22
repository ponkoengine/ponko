//! Key labels from an xkb keymap: the keyboard layouts of Linux and the
//! BSDs, on X11 and Wayland alike.
//!
//! A Wayland compositor hands every client its keymap as text (the format
//! `xkbcomp` and libxkbcommon write), and an X server gives the same
//! table through its XKB extension (`GetMap`). Either way an [`XkbKeymap`]
//! keeps, for each character key, the symbol its first shift level types
//! in each layout (xkb's "groups"); [`XkbKeymap::labels`] turns the active
//! group into [`KeyLabels`]. This part needs no display, so it is tested
//! on every system; reading the keymap from a session is in `watch.rs`.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::{KeyLabels, LAYOUT_KEYS};

/// X11 and xkb key codes are the Linux evdev codes plus 8; for the
/// character keys the evdev code is the PC scan code in [`LAYOUT_KEYS`].
const EVDEV_OFFSET: u32 = 8;

/// What a key does when the active group is past its last one (xkb's
/// `GroupsWrap`, `GroupsClamp`, `GroupsRedirect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OutOfRange {
    #[default]
    Wrap,
    Clamp,
    Redirect(u32),
}

/// One key's first-level character in each group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct KeyGroups {
    groups: Vec<Option<char>>,
    out_of_range: OutOfRange,
}

impl KeyGroups {
    fn get(&self, group: u32) -> Option<char> {
        let n = self.groups.len() as u32;
        if n == 0 {
            return None;
        }
        let group = if group < n {
            group
        } else {
            match self.out_of_range {
                OutOfRange::Wrap => group % n,
                OutOfRange::Clamp => n - 1,
                OutOfRange::Redirect(to) if to < n => to,
                OutOfRange::Redirect(_) => 0,
            }
        };
        self.groups[group as usize]
    }
}

/// The character keys of an xkb keymap: what each types in each layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct XkbKeymap {
    /// By [`LAYOUT_KEYS`] index.
    keys: Vec<KeyGroups>,
}

impl XkbKeymap {
    fn empty() -> Self {
        Self {
            keys: vec![KeyGroups::default(); LAYOUT_KEYS.len()],
        }
    }

    /// The [`LAYOUT_KEYS`] index of an X11/xkb key code.
    fn index_of(keycode: u32) -> Option<usize> {
        LAYOUT_KEYS
            .iter()
            .position(|&(_, _, scan)| u32::from(scan) + EVDEV_OFFSET == keycode)
    }

    /// How many layouts (groups) the keymap has: the most any key has.
    #[cfg(test)]
    fn group_count(&self) -> u32 {
        self.keys.iter().map(|k| k.groups.len() as u32).max().unwrap_or(0)
    }

    /// The labels with layout `group` (from 0) active.
    pub(crate) fn labels(&self, group: u32) -> KeyLabels {
        let mut labels = KeyLabels::us();
        for ((key, ..), syms) in LAYOUT_KEYS.iter().zip(&self.keys) {
            if let Some(c) = syms.get(group) {
                labels.set(*key, c);
            }
        }
        labels
    }

    /// A keymap from an X server's XKB `GetMap` reply: the key symbol maps
    /// from `first_keycode` on, each as (`group_info`, `width`, symbols):
    /// the low nibble of `group_info` counts the groups, its top bits say
    /// what an out-of-range group does, and the symbols are `width` levels
    /// per group.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_x11<'a>(first_keycode: u8, maps: impl IntoIterator<Item = (u8, u8, &'a [u32])>) -> Self {
        let mut keymap = Self::empty();
        for (i, (info, width, syms)) in maps.into_iter().enumerate() {
            let Some(index) = Self::index_of(u32::from(first_keycode) + i as u32) else {
                continue;
            };
            let n = u32::from(info & 0x0F);
            let width = usize::from(width);
            let groups = (0..n as usize)
                .map(|g| syms.get(g * width).and_then(|&s| keysym_char(s)))
                .collect();
            let out_of_range = match info >> 6 {
                1 => OutOfRange::Clamp,
                2 => OutOfRange::Redirect(u32::from((info >> 4) & 0x3)),
                _ => OutOfRange::Wrap,
            };
            keymap.keys[index] = KeyGroups { groups, out_of_range };
        }
        keymap
    }

    /// Reads a keymap's text (`xkb_keymap { xkb_keycodes ...; xkb_symbols
    /// ...; }`, as a Wayland compositor sends it). `None` when it has no
    /// symbols section.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Parser::new(text).keymap()
    }
}

/// A token of the xkb text format.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token<'a> {
    /// A word: a section or field name, a keysym name, a number.
    Word(&'a str),
    /// A key name: `<AD01>`.
    KeyName(&'a str),
    /// A quoted string.
    Text(&'a str),
    /// `{ } [ ] = , ; ( ) + - !` and the like.
    Punct(char),
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'<' => {
                let end = text[i + 1..].find('>').map_or(bytes.len(), |e| i + 1 + e);
                out.push(Token::KeyName(&text[i + 1..end]));
                i = end + 1;
            }
            b'"' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                out.push(Token::Text(&text[i + 1..end.min(bytes.len())]));
                i = end + 1;
            }
            _ if c.is_ascii_alphanumeric() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Token::Word(&text[start..i]));
            }
            _ => {
                let ch = text[i..].chars().next().unwrap_or(' ');
                out.push(Token::Punct(ch));
                i += ch.len_utf8();
            }
        }
    }
    out
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            tokens: tokens(text),
            at: 0,
        }
    }

    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token<'a>> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Token::Punct(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Skips one statement: up to and past the `;` (or `,` with `comma`)
    /// at this depth, or up to a closing bracket of an enclosing level,
    /// which is left in place.
    fn skip_item(&mut self, comma: bool) {
        let mut depth = 0usize;
        while let Some(t) = self.peek() {
            match t {
                Token::Punct('{' | '[' | '(') => depth += 1,
                Token::Punct('}' | ']' | ')') => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Token::Punct(';') if depth == 0 => {
                    self.at += 1;
                    return;
                }
                Token::Punct(',') if depth == 0 && comma => {
                    self.at += 1;
                    return;
                }
                _ => {}
            }
            self.at += 1;
        }
    }

    /// Skips a `{ ... }` block (the `{` is next).
    fn skip_block(&mut self) {
        if !self.eat('{') {
            return;
        }
        let mut depth = 1usize;
        while let Some(t) = self.next() {
            match t {
                Token::Punct('{') => depth += 1,
                Token::Punct('}') => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn keymap(&mut self) -> Option<XkbKeymap> {
        let mut codes: HashMap<&str, u32> = HashMap::new();
        let mut aliases: HashMap<&str, &str> = HashMap::new();
        let mut symbols: Vec<(&str, KeyGroups)> = Vec::new();
        let mut seen_symbols = false;
        while let Some(t) = self.next() {
            let Token::Word(word) = t else { continue };
            match word {
                "xkb_keycodes" => {
                    self.to_block();
                    self.keycodes(&mut codes, &mut aliases);
                }
                "xkb_symbols" => {
                    self.to_block();
                    self.symbols(&mut symbols, &mut aliases);
                    seen_symbols = true;
                }
                "xkb_types" | "xkb_compatibility" | "xkb_compat" | "xkb_geometry" => {
                    self.to_block();
                    self.skip_block();
                }
                _ => {}
            }
        }
        if !seen_symbols {
            return None;
        }
        let mut keymap = XkbKeymap::empty();
        for (name, groups) in symbols {
            let mut name = name;
            for _ in 0..8 {
                match aliases.get(name) {
                    Some(real) => name = real,
                    None => break,
                }
            }
            let code = codes.get(name).copied().or_else(|| evdev_name_code(name));
            if let Some(index) = code.and_then(XkbKeymap::index_of) {
                keymap.keys[index] = groups;
            }
        }
        Some(keymap)
    }

    /// Moves to a section's `{` (past its optional name).
    fn to_block(&mut self) {
        while let Some(t) = self.peek() {
            if *t == Token::Punct('{') {
                return;
            }
            self.at += 1;
        }
    }

    fn keycodes(&mut self, codes: &mut HashMap<&'a str, u32>, aliases: &mut HashMap<&'a str, &'a str>) {
        if !self.eat('{') {
            return;
        }
        loop {
            match self.peek().cloned() {
                None => return,
                Some(Token::Punct('}')) => {
                    self.at += 1;
                    return;
                }
                Some(Token::KeyName(name)) => {
                    self.at += 1;
                    if self.eat('=')
                        && let Some(Token::Word(n)) = self.peek().cloned()
                        && let Ok(code) = n.parse()
                    {
                        codes.insert(name, code);
                    }
                    self.skip_item(false);
                }
                Some(Token::Word("alias")) => {
                    self.at += 1;
                    if let Some(Token::KeyName(alias)) = self.next()
                        && self.eat('=')
                        && let Some(Token::KeyName(real)) = self.next()
                    {
                        aliases.insert(alias, real);
                    }
                    self.skip_item(false);
                }
                Some(_) => {
                    self.skip_item(false);
                    // A stray closer of a nested block: step over it.
                    if matches!(self.peek(), Some(Token::Punct(']' | ')'))) {
                        self.at += 1;
                    }
                }
            }
        }
    }

    fn symbols(&mut self, symbols: &mut Vec<(&'a str, KeyGroups)>, aliases: &mut HashMap<&'a str, &'a str>) {
        if !self.eat('{') {
            return;
        }
        loop {
            match self.peek().cloned() {
                None => return,
                Some(Token::Punct('}')) => {
                    self.at += 1;
                    return;
                }
                Some(Token::Word("key")) => {
                    self.at += 1;
                    if let Some(Token::KeyName(name)) = self.next() {
                        let groups = self.key_body();
                        if let Some(old) = symbols.iter_mut().find(|(n, _)| *n == name) {
                            old.1 = groups;
                        } else {
                            symbols.push((name, groups));
                        }
                    }
                    self.skip_item(false);
                }
                Some(Token::Word("alias")) => {
                    self.at += 1;
                    if let Some(Token::KeyName(alias)) = self.next()
                        && self.eat('=')
                        && let Some(Token::KeyName(real)) = self.next()
                    {
                        aliases.insert(alias, real);
                    }
                    self.skip_item(false);
                }
                Some(_) => {
                    self.skip_item(false);
                    if matches!(self.peek(), Some(Token::Punct(']' | ')'))) {
                        self.at += 1;
                    }
                }
            }
        }
    }

    /// `[Group2]` (or `[2]`): the group's index from 0.
    fn group_index(&mut self) -> Option<usize> {
        if !self.eat('[') {
            return None;
        }
        let index = match self.next() {
            Some(Token::Word(w)) => {
                let digits = w.trim_start_matches(|c: char| c.is_ascii_alphabetic());
                digits.parse::<usize>().ok().filter(|&n| n >= 1).map(|n| n - 1)
            }
            _ => None,
        };
        self.eat(']');
        index
    }

    /// A key's `{ ... }`: its groups' first-level symbols.
    fn key_body(&mut self) -> KeyGroups {
        let mut key = KeyGroups::default();
        if !self.eat('{') {
            return key;
        }
        let mut implicit = 0usize;
        loop {
            match self.peek().cloned() {
                None => return key,
                Some(Token::Punct('}')) => {
                    self.at += 1;
                    return key;
                }
                Some(Token::Punct(',')) => self.at += 1,
                // A bare list: the next group.
                Some(Token::Punct('[')) => {
                    let first = self.level_list();
                    set_group(&mut key.groups, implicit, first);
                    implicit += 1;
                }
                Some(Token::Word(w)) if w.eq_ignore_ascii_case("symbols") => {
                    self.at += 1;
                    let group = self.group_index().unwrap_or(implicit);
                    if self.eat('=') && self.peek() == Some(&Token::Punct('[')) {
                        let first = self.level_list();
                        set_group(&mut key.groups, group, first);
                        implicit = implicit.max(group + 1);
                    } else {
                        self.skip_item(true);
                    }
                }
                Some(Token::Word(w))
                    if w.eq_ignore_ascii_case("groupswrap") || w.eq_ignore_ascii_case("wrapgroups") =>
                {
                    key.out_of_range = OutOfRange::Wrap;
                    self.skip_item(true);
                }
                Some(Token::Word(w))
                    if w.eq_ignore_ascii_case("groupsclamp") || w.eq_ignore_ascii_case("clampgroups") =>
                {
                    key.out_of_range = OutOfRange::Clamp;
                    self.skip_item(true);
                }
                Some(Token::Word(w))
                    if w.eq_ignore_ascii_case("groupsredirect") || w.eq_ignore_ascii_case("redirectgroups") =>
                {
                    self.at += 1;
                    if self.eat('=')
                        && let Some(Token::Word(g)) = self.next()
                    {
                        let digits = g.trim_start_matches(|c: char| c.is_ascii_alphabetic());
                        if let Ok(n) = digits.parse::<u32>() {
                            key.out_of_range = OutOfRange::Redirect(n.saturating_sub(1));
                        }
                    }
                    self.skip_item(true);
                }
                Some(_) => self.skip_item(true),
            }
        }
    }

    /// A `[ level1, level2, ... ]` list (the `[` is next): the first
    /// level's character. A level may hold several symbols (`{ a, b }`):
    /// the first one counts.
    fn level_list(&mut self) -> Option<char> {
        self.eat('[');
        let mut first = None;
        let mut level = 0usize;
        let mut depth = 0usize;
        while let Some(t) = self.next() {
            match t {
                Token::Punct(']') if depth == 0 => break,
                Token::Punct('{') => depth += 1,
                Token::Punct('}') => depth = depth.saturating_sub(1),
                Token::Punct(',') if depth == 0 => level += 1,
                Token::Word(w) if level == 0 && first.is_none() => {
                    first = Some(keysym_word_char(w));
                }
                _ => {}
            }
        }
        first.flatten()
    }
}

fn set_group(groups: &mut Vec<Option<char>>, group: usize, c: Option<char>) {
    if group >= 4 {
        return; // xkb has four groups at most
    }
    if groups.len() <= group {
        groups.resize(group + 1, None);
    }
    groups[group] = c;
}

/// The xkb key code of a standard evdev key name, for a keymap without a
/// keycodes section.
fn evdev_name_code(name: &str) -> Option<u32> {
    const ROWS: [(&str, u32, u32); 4] = [("AE", 2, 12), ("AD", 16, 12), ("AC", 30, 11), ("AB", 44, 10)];
    match name {
        // The key left of 1, and the backslash key (`<AC12>` on ISO boards).
        "TLDE" => return Some(41 + EVDEV_OFFSET),
        "BKSL" | "AC12" => return Some(43 + EVDEV_OFFSET),
        _ => {}
    }
    let (row, col) = name.split_at_checked(2)?;
    let col: u32 = col.parse().ok()?;
    let &(_, first, count) = ROWS.iter().find(|r| r.0 == row)?;
    (1..=count).contains(&col).then(|| first + col - 1 + EVDEV_OFFSET)
}

/// The character a keysym written as a word types: a name (`eacute`,
/// `Cyrillic_ef`, `dead_circumflex`), `U00E9`, `0x1000434` or a digit.
fn keysym_word_char(word: &str) -> Option<char> {
    if word.len() == 1 && word.as_bytes()[0].is_ascii_digit() {
        return word.chars().next();
    }
    if let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(keysym_char);
    }
    if word.as_bytes()[0].is_ascii_digit() {
        return word.parse().ok().and_then(keysym_char);
    }
    if let Some(hex) = word.strip_prefix('U')
        && (4..=6).contains(&hex.len())
        && let Ok(code) = u32::from_str_radix(hex, 16)
    {
        return char::from_u32(code);
    }
    if let Some(dead) = word.strip_prefix("dead_") {
        return dead_char(dead);
    }
    keysym_names().get(word).copied().and_then(keysym_char)
}

/// The character a keysym value types, dead keys giving their own mark.
fn keysym_char(keysym: u32) -> Option<char> {
    if (0xFE50..=0xFE8F).contains(&keysym) {
        let name = xkeysym::Keysym::new(keysym).name()?;
        return dead_char(name.strip_prefix("XK_dead_")?);
    }
    xkeysym::Keysym::new(keysym).key_char().filter(|c| !c.is_control())
}

/// A dead key's own mark, as it reads on the key.
fn dead_char(name: &str) -> Option<char> {
    Some(match name {
        "grave" => '`',
        "acute" => '´',
        "circumflex" => '^',
        "tilde" | "perispomeni" => '~',
        "macron" => '¯',
        "breve" => '˘',
        "abovedot" => '˙',
        "diaeresis" => '¨',
        "abovering" => '˚',
        "doubleacute" => '˝',
        "caron" => 'ˇ',
        "cedilla" => '¸',
        "ogonek" => '˛',
        "iota" => 'ͺ',
        "belowdot" => '.',
        "hook" => '̉',
        "horn" => '̛',
        "stroke" => '/',
        "psili" | "abovecomma" => '᾿',
        "dasia" | "abovereversedcomma" => '῾',
        "currency" => '¤',
        "greek" => 'µ',
        _ => return None,
    })
}

/// Keysym names (without `XK_`) to values, for the named keysyms that
/// can be on a character key: the legacy range and the named Unicode ones.
fn keysym_names() -> &'static HashMap<&'static str, u32> {
    static NAMES: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = HashMap::with_capacity(2600);
        for raw in (0x0000..=0xFFFFu32).chain(0x0100_0000..=0x0100_3000) {
            if let Some(name) = xkeysym::Keysym::new(raw).name()
                && let Some(name) = name.strip_prefix("XK_")
            {
                names.entry(name).or_insert(raw);
            }
        }
        names
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    /// A US keymap as libxkbcommon writes it (`xkb_keymap_get_as_string`),
    /// cut to a few keys; the types and compat sections stay, to be skipped.
    const US: &str = r#"xkb_keymap {
xkb_keycodes "evdev+aliases(qwerty)" {
	minimum = 8;
	maximum = 255;
	<ESC>                = 9;
	<AE01>               = 10;
	<AE02>               = 11;
	<AD01>               = 24;
	<AD02>               = 25;
	<AD06>               = 29;
	<AC01>               = 38;
	<AC10>               = 47;
	<AB01>               = 52;
	<AB07>               = 58;
	<AE11>               = 20;
	<TLDE>               = 49;
	<BKSL>               = 51;
	indicator 1 = "Caps Lock";
	alias <AC12>         = <BKSL>;
};
xkb_types "complete" {
	virtual_modifiers NumLock,Alt;
	type "ALPHABETIC" {
		modifiers= Shift+Lock;
		map[Shift]= 2;
		map[Lock]= 2;
		level_name[1]= "Base";
		level_name[2]= "Caps";
	};
};
xkb_compatibility "complete" {
	interpret Shift_Lock+AnyOf(Shift+Lock) {
		action= LockMods(modifiers=Shift);
	};
	indicator "Caps Lock" { whichModState= locked; modifiers= Lock; };
};
xkb_symbols "pc+us+inet(evdev)" {
	name[Group1]="English (US)";
	key <ESC>                {	[          Escape ] };
	key <AE01>               {	[               1,          exclam ] };
	key <AE02>               {	[               2,              at ] };
	key <AE11>               {	[           minus,      underscore ] };
	key <AD01>               {
		type= "ALPHABETIC",
		symbols[Group1]= [               q,               Q ]
	};
	key <AD02>               {	[               w,               W ] };
	key <AD06>               {	[               y,               Y ] };
	key <AC01>               {	[               a,               A ] };
	key <AC10>               {	[       semicolon,           colon ] };
	key <AB01>               {	[               z,               Z ] };
	key <AB07>               {	[               m,               M ] };
	key <TLDE>               {	[           grave,      asciitilde ] };
	key <AC12>               {	[       backslash,             bar ] };
	modifier_map Shift { <LFSH>, <RTSH> };
};
};
"#;

    /// A keymap with two layouts, French (AZERTY) then German (QWERTZ), as
    /// `setxkbmap fr,de` gives it: both groups on each key.
    const FR_DE: &str = r#"xkb_keymap {
xkb_keycodes "evdev+aliases(azerty)" {
	<AE01> = 10; <AE02> = 11; <AE11> = 20; <AD01> = 24; <AD02> = 25; <AD06> = 29;
	<AD11> = 34; <AC01> = 38; <AC10> = 47; <AC11> = 48; <AB01> = 52; <AB07> = 58;
	<AB10> = 61;
};
xkb_types "complete" { type "FOUR_LEVEL" { modifiers= Shift+LevelThree; }; };
xkb_symbols "pc+fr+de:2+inet(evdev)" {
	name[Group1]="French";
	name[Group2]="German";
	key <AE01> {
		type[Group1]= "FOUR_LEVEL_ALPHABETIC",
		symbols[Group1]= [ ampersand, 1, onesuperior, exclamdown ],
		symbols[Group2]= [ 1, exclam, onesuperior, exclamdown ]
	};
	key <AE02> { symbols[Group1]= [ eacute, 2, asciitilde, oneeighth ], symbols[Group2]= [ 2, quotedbl ] };
	key <AE11> { symbols[Group1]= [ parenright, degree ], symbols[Group2]= [ ssharp, question ] };
	key <AD01> { symbols[Group1]= [ a, A ], symbols[Group2]= [ q, Q, at ] };
	key <AD02> { symbols[Group1]= [ z, Z ], symbols[Group2]= [ w, W ] };
	key <AD06> { symbols[Group1]= [ y, Y ], symbols[Group2]= [ z, Z ] };
	key <AD11> { symbols[Group1]= [ dead_circumflex, dead_diaeresis ], symbols[Group2]= [ udiaeresis, Udiaeresis ] };
	key <AC01> { symbols[Group1]= [ q, Q ], symbols[Group2]= [ a, A ] };
	key <AC10> { symbols[Group1]= [ m, M ], symbols[Group2]= [ odiaeresis, Odiaeresis ] };
	key <AC11> { symbols[Group1]= [ ugrave, percent ], symbols[Group2]= [ adiaeresis, Adiaeresis ] };
	key <AB01> { symbols[Group1]= [ w, W ], symbols[Group2]= [ y, Y ] };
	key <AB07> { symbols[Group1]= [ comma, question ], symbols[Group2]= [ m, M ] };
	key <AB10> { symbols[Group1]= [ exclam, section ], symbols[Group2]= [ minus, underscore ] };
};
};
"#;

    fn row(labels: &KeyLabels, keys: &[Key]) -> Vec<String> {
        keys.iter().map(|&k| labels.label(k).to_string()).collect()
    }

    const KEYS: [Key; 9] = [
        Key::Q,
        Key::W,
        Key::Y,
        Key::A,
        Key::Z,
        Key::Num1,
        Key::Num2,
        Key::Semicolon,
        Key::M,
    ];

    #[test]
    fn a_us_keymap_gives_the_us_labels() {
        let keymap = XkbKeymap::parse(US).expect("a keymap");
        assert_eq!(keymap.labels(0), KeyLabels::us());
        assert_eq!(keymap.group_count(), 1);
    }

    #[test]
    fn each_layout_of_a_two_layout_keymap_labels_the_keys_as_printed() {
        let keymap = XkbKeymap::parse(FR_DE).expect("a keymap");
        assert_eq!(keymap.group_count(), 2);
        let french = keymap.labels(0);
        assert_eq!(row(&french, &KEYS), ["A", "Z", "Y", "Q", "W", "&", "É", "M", ","]);
        assert_eq!(french.label(Key::LeftBracket), "^", "a dead key reads as its mark");
        assert_eq!(french.label(Key::Apostrophe), "Ù");
        assert_eq!(french.label(Key::Slash), "!");
        let german = keymap.labels(1);
        assert_eq!(row(&german, &KEYS), ["Q", "W", "Z", "A", "Y", "1", "2", "Ö", "M"]);
        assert_eq!(german.label(Key::Minus), "ß");
        assert_eq!(german.label(Key::LeftBracket), "Ü");
        assert_eq!(german.label(Key::Slash), "-");
        // Keys the keymap leaves out keep the US labels.
        assert_eq!(german.label(Key::Period), ".");
        assert_eq!(german.label(Key::Space), "Space");
    }

    #[test]
    fn a_group_past_a_keys_last_wraps_like_xkb() {
        let keymap = XkbKeymap::parse(FR_DE).expect("a keymap");
        assert_eq!(keymap.labels(2), keymap.labels(0));
        assert_eq!(keymap.labels(3), keymap.labels(1));
    }

    #[test]
    fn bare_group_lists_keysym_spellings_and_aliases_are_understood() {
        let text = r#"
            // A hand-written keymap: no keycodes section (standard evdev names),
            // bare lists for the groups, every way to write a symbol.
            xkb_keymap { xkb_symbols "x" {
                key <AD01> { [ Cyrillic_shorti, Cyrillic_SHORTI ], [ q, Q ] };
                key <AD02> { [ U0446 ], [ 0x77 ] };
                key <AE03> { [ 3, numbersign ] };
                key <AC02> { [ { s, t }, S ] };
                key <AE04> { type = "TWO_LEVEL", [ NoSymbol, dollar ] };
                key <AC12> { [ dead_acute ] };
                key <TLDE> { [ Cyrillic_io ] };
                key <AE05> { groupsClamp, symbols[Group1] = [ 5 ], symbols[Group2] = [ percent ] };
            }; };
        "#;
        let keymap = XkbKeymap::parse(text).expect("a keymap");
        let cyrillic = keymap.labels(0);
        let latin = keymap.labels(1);
        assert_eq!(
            [Key::Q, Key::W, Key::Num3, Key::S, Key::Num4, Key::Backslash].map(|k| cyrillic.label(k)),
            ["Й", "Ц", "3", "S", "4", "´"]
        );
        assert_eq!(cyrillic.label(Key::Backquote), "Ё");
        assert_eq!(latin.label(Key::Q), "Q");
        assert_eq!(latin.label(Key::W), "W");
        assert_eq!(latin.label(Key::Num3), "3", "one group: group 2 wraps to it");
        assert_eq!(keymap.labels(3).label(Key::Num5), "%", "clamped to the last group");
    }

    #[test]
    fn text_without_a_symbols_section_is_not_a_keymap() {
        assert_eq!(XkbKeymap::parse(""), None);
        assert_eq!(XkbKeymap::parse("xkb_keymap { xkb_keycodes { <AD01> = 24; }; };"), None);
        // Broken text still gives what it can, without a panic.
        let broken = XkbKeymap::parse("xkb_symbols { key <AD01> { [ a, A ] }; key <AD02> { [ z").expect("a keymap");
        assert_eq!(broken.labels(0).label(Key::Q), "A");
    }

    #[test]
    fn an_x_servers_key_symbol_maps_give_the_same_labels() {
        // From key code 24 (Q's place): Q, W, E with French and German
        // groups (two levels each); E has one group only.
        let (a, z, q, w) = (0x61, 0x7A, 0x71, 0x77);
        let q_key = [a, 0x41, q, 0x51];
        let w_key = [z, 0x5A, w, 0x57];
        let e_key = [0x65, 0x45];
        let maps = [(2u8, 2u8, &q_key[..]), (2, 2, &w_key[..]), (1, 2, &e_key[..])];
        let keymap = XkbKeymap::from_x11(24, maps);
        assert_eq!(keymap.group_count(), 2);
        let (french, german) = (keymap.labels(0), keymap.labels(1));
        assert_eq!([Key::Q, Key::W, Key::E].map(|k| french.label(k)), ["A", "Z", "E"]);
        assert_eq!([Key::Q, Key::W, Key::E].map(|k| german.label(k)), ["Q", "W", "E"]);
        // Clamp (0x40) instead of wrap, for a third group.
        let keymap = XkbKeymap::from_x11(24, [(0x42u8, 2u8, &q_key[..])]);
        assert_eq!(keymap.labels(2).label(Key::Q), "Q");
    }

    #[test]
    fn keysyms_read_as_the_character_they_type() {
        assert_eq!(keysym_word_char("eacute"), Some('é'));
        assert_eq!(keysym_word_char("ssharp"), Some('ß'));
        assert_eq!(keysym_word_char("Greek_alpha"), Some('α'));
        assert_eq!(keysym_word_char("Armenian_ayb"), Some('ա'));
        assert_eq!(keysym_word_char("dead_grave"), Some('`'));
        assert_eq!(keysym_word_char("7"), Some('7'));
        assert_eq!(keysym_word_char("U20AC"), Some('€'));
        assert_eq!(keysym_word_char("0x10020ac"), Some('€'));
        assert_eq!(keysym_word_char("NoSymbol"), None);
        assert_eq!(keysym_word_char("Escape"), None, "a control character is no label");
        assert_eq!(keysym_char(0xFE52), Some('^'), "dead_circumflex by value");
    }
}
