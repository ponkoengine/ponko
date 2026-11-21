//! Rich text in retained UI labels ([`Ui::set_rich`]): a small markup for
//! spans of color, weight, font, size and underline, inline images (button
//! glyphs) and clickable ranges (links). The markup is parsed once, when the
//! node's text changes, into the text that is laid out (each image a
//! U+FFFC object replacement character), the text a screen reader reads
//! (each image its alternative text) and the spans over it. Laying it out
//! is allocation-free in steady state: it measures runs with the engine's
//! text layout, wraps at the line-break opportunities found when parsing,
//! lines up the runs' baselines and, with `text-shaping`, puts the runs of
//! each line in visual order (UAX #9, rule L2) with the embedding levels
//! found when parsing.

use super::retained::*;
use crate::{camera::Rect, color::Color, text::Font, texture::Texture};
use glam::Vec2;
use std::collections::HashMap;

/// What stands for an inline image in the text that is laid out.
pub(crate) const OBJECT: char = '\u{FFFC}';

/// How a run of rich text is drawn: what its markup changed from the node's
/// paint.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Style {
    pub color: Option<Color>,
    pub font: Option<Font>,
    pub size: Option<f32>,
    /// Bold with no bold font named: drawn twice, a little apart.
    pub bold: bool,
    pub underline: bool,
    /// The index of the link it belongs to.
    pub link: Option<u32>,
}

/// A run of the laid-out text with one style, or one inline image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Span {
    pub start: u32,
    pub end: u32,
    pub style: Style,
    pub image: Option<Texture>,
    /// Its bidirectional embedding level (0 without `text-shaping`).
    pub level: u8,
}

/// A node's markup, parsed.
#[derive(Debug, Default)]
pub(crate) struct RichText {
    /// The text laid out: the markup without its tags, each image U+FFFC.
    pub text: String,
    /// What a screen reader reads: each image as its alternative text.
    pub plain: String,
    /// Runs covering `text` in order, without gaps.
    pub spans: Vec<Span>,
    /// Each link's target (`[link=target]`), by index.
    pub links: Vec<String>,
    /// Byte offsets in `text` where a line may end (not paragraph ends).
    pub breaks: Vec<u32>,
    /// The [`RichAssets`] revision it was parsed with; 0: parse it again.
    pub parsed: u64,
}

/// The named fonts and images markup refers to ([`Ui::set_inline_font`],
/// [`Ui::set_inline_image`]).
#[derive(Debug)]
pub(crate) struct RichAssets {
    fonts: HashMap<String, Font>,
    images: HashMap<String, Texture>,
    /// Goes up when a name is set: rich nodes are parsed again.
    pub revision: u64,
}

impl Default for RichAssets {
    fn default() -> Self {
        Self {
            fonts: HashMap::new(),
            images: HashMap::new(),
            revision: 1,
        }
    }
}

/// One run or image placed on a line, in units of the layout (logical UI
/// units times the UI scale), from the text box's top left corner.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Item {
    /// Where its text is drawn from (the top of its font's line box), or
    /// its image's top left corner.
    pub at: Vec2,
    /// Its width; an image's height.
    pub width: f32,
    pub image_height: f32,
    /// Its line's width, and how far along the room left beside the line
    /// the line goes: 0 at the box's left edge, 0.5 centred, 1 at its
    /// right edge ([`Item::line_x`]).
    pub line: f32,
    pub along: f32,
    /// The line's baseline, top and bottom.
    pub baseline: f32,
    pub top: f32,
    pub bottom: f32,
    /// Bytes of `RichText::text`.
    pub start: u32,
    pub end: u32,
    /// Index of its span.
    pub span: u32,
}

impl Item {
    /// How far right of where it was laid out its line goes in a box
    /// `width` wide: its alignment, for a layout made without a box width
    /// ([`Align::width`] `None`).
    pub(crate) fn line_x(&self, width: f32) -> f32 {
        self.along * (width - self.line)
    }
}

/// Scratch space for laying rich text out, reused frame to frame.
#[derive(Debug, Default)]
pub(crate) struct RichLayout {
    pub items: Vec<Item>,
    /// The width it was wrapped at (`0` unwrapped), and the range of
    /// widths that wrap it the same way: give the same items.
    pub room: crate::text::WrapRoom,
}

/// The node's paint the markup changes, and how to measure.
#[derive(Clone, Copy)]
pub(crate) struct Base {
    pub font: Font,
    /// The font size in logical UI units.
    pub size: f32,
    /// The UI scale: sizes and the result are multiplied by it.
    pub scale: f32,
    /// Fonts the markup names are used (else every run is measured in
    /// `font`, as a UI with no graphics measures).
    pub fonts: bool,
    /// Physical pixels per unit of `size × scale` (the window's scale
    /// factor when drawing): text is measured at physical pixels.
    pub pixels: f32,
}

/// How lines are placed across the text box.
#[derive(Clone, Copy)]
pub(crate) struct Align {
    pub align: UiTextAlign,
    /// The text box's width. `None` starts every line at the left edge,
    /// for its drawing to align it in whatever width the box has then
    /// ([`Item::line_x`]): a node that moves and whose width changes in
    /// its last bits keeps its layout.
    pub width: Option<f32>,
    /// Start and end follow each paragraph's direction.
    pub directional: bool,
}

impl Align {
    pub(crate) const MEASURE: Align = Align {
        align: UiTextAlign::Start,
        width: None,
        directional: false,
    };

    /// How far along the room left beside a line it goes: 0 at the box's
    /// left edge, 1 at its right. Start and End follow a right-to-left
    /// paragraph's direction.
    fn along(self, rtl: bool) -> f32 {
        match (self.align, rtl) {
            (UiTextAlign::Start, false) | (UiTextAlign::End, true) => 0.0,
            (UiTextAlign::Center, _) => 0.5,
            (UiTextAlign::End, false) | (UiTextAlign::Start, true) => 1.0,
        }
    }
}

/// Recognized tag names, in the order of [`Tag`].
const TAGS: [&str; 7] = ["color", "b", "u", "font", "size", "link", "img"];

impl RichAssets {
    pub(crate) fn set_font(&mut self, name: &str, font: Font) {
        if self.fonts.get(name) != Some(&font) {
            self.fonts.insert(name.to_string(), font);
            self.revision += 1;
        }
    }
    pub(crate) fn set_image(&mut self, name: &str, texture: Texture) {
        if self.images.get(name) != Some(&texture) {
            self.images.insert(name.to_string(), texture);
            self.revision += 1;
        }
    }
}

/// The next `key` or `key=value` of a tag's contents; a value may be quoted.
fn next_pair<'a>(s: &mut &'a str) -> Option<(&'a str, Option<&'a str>)> {
    let t = s.trim_start();
    if t.is_empty() {
        *s = t;
        return None;
    }
    let key_end = t.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(t.len());
    let key = &t[..key_end];
    let rest = &t[key_end..];
    let Some(value) = rest.strip_prefix('=') else {
        *s = rest;
        return Some((key, None));
    };
    if let Some(quoted) = value.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        *s = quoted.get(end + 1..).unwrap_or("");
        return Some((key, Some(&quoted[..end])));
    }
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    *s = &value[end..];
    Some((key, Some(&value[..end])))
}

/// A tag's contents (between the brackets) and its length with them, for
/// `s` starting at `[`; `None` when no `]` closes it on this line.
fn bracket(s: &str) -> Option<(&str, usize)> {
    let mut quoted = false;
    for (i, c) in s.char_indices().skip(1) {
        match c {
            '"' => quoted = !quoted,
            ']' if !quoted => return Some((&s[1..i], i + 1)),
            '[' | '\n' if !quoted => return None,
            _ => {}
        }
    }
    None
}

/// `#rgb`, `#rrggbb`, `#rrggbbaa` or a few names.
fn parse_color(value: &str) -> Option<Color> {
    let named = match value.to_ascii_lowercase().as_str() {
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        "gray" | "grey" => Some(Color::GRAY),
        "red" => Some(Color::RED),
        "green" => Some(Color::GREEN),
        "blue" => Some(Color::BLUE),
        "yellow" => Some(Color::YELLOW),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let hex = value.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        3 => {
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some(Color::hex(((r * 17) << 16) | ((g * 17) << 8) | (b * 17)))
        }
        6 => Some(Color::hex(v)),
        8 => Some(Color::hex(v >> 8).with_alpha((v & 0xff) as f32 / 255.0)),
        _ => None,
    }
}

/// Escapes `text` so markup shows it as it is: each `[` doubled.
pub(crate) fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        if c == '[' {
            out.push('[');
        }
        out.push(c);
    }
}

impl RichText {
    /// Adds `s` in `style`, extending the last span when it matches.
    fn push_text(&mut self, s: &str, style: Style) {
        if s.is_empty() {
            return;
        }
        let start = self.text.len() as u32;
        self.text.push_str(s);
        self.plain.push_str(s);
        let end = self.text.len() as u32;
        if let Some(last) = self.spans.last_mut()
            && last.image.is_none()
            && last.style == style
            && last.end == start
        {
            last.end = end;
            return;
        }
        self.spans.push(Span {
            start,
            end,
            style,
            image: None,
            level: 0,
        });
    }

    /// Parses `markup` (see [`Ui::set_rich`]) with the names in `assets`.
    pub(crate) fn parse(&mut self, markup: &str, assets: &RichAssets) {
        self.text.clear();
        self.plain.clear();
        self.spans.clear();
        self.links.clear();
        self.breaks.clear();
        self.parsed = assets.revision;
        let mut style = Style::default();
        // Each open tag: its name and the style before it.
        let mut open: Vec<(&'static str, Style)> = Vec::new();
        let mut rest = markup;
        while let Some(i) = rest.find('[') {
            self.push_text(&rest[..i], style);
            rest = &rest[i..];
            if let Some(after) = rest.strip_prefix("[[") {
                self.push_text("[", style);
                rest = after;
                continue;
            }
            let consumed =
                bracket(rest).and_then(|(inside, len)| self.tag(inside, assets, &mut style, &mut open).then_some(len));
            match consumed {
                Some(len) => rest = &rest[len..],
                None => {
                    // Not markup: shown as it is.
                    self.push_text("[", style);
                    rest = &rest[1..];
                }
            }
        }
        self.push_text(rest, style);
        self.find_breaks();
        self.find_levels();
    }

    /// Applies one tag's contents; false when it isn't markup this knows.
    fn tag(
        &mut self,
        inside: &str,
        assets: &RichAssets,
        style: &mut Style,
        open: &mut Vec<(&'static str, Style)>,
    ) -> bool {
        if let Some(name) = inside.strip_prefix('/') {
            let name = name.trim();
            let Some(at) = open.iter().rposition(|(open, _)| open.eq_ignore_ascii_case(name)) else {
                return false;
            };
            *style = open[at].1;
            open.truncate(at);
            return true;
        }
        let mut s = inside;
        let Some((name, value)) = next_pair(&mut s) else {
            return false;
        };
        let Some(&tag) = TAGS.iter().find(|t| t.eq_ignore_ascii_case(name)) else {
            return false;
        };
        let before = *style;
        match (tag, value) {
            ("color", Some(v)) => match parse_color(v) {
                Some(c) => style.color = Some(c),
                None => return false,
            },
            ("b", None) => match assets.fonts.get("bold") {
                Some(&font) => style.font = Some(font),
                None => style.bold = true,
            },
            ("u", None) => style.underline = true,
            // A name not set (yet) changes nothing: the node is parsed
            // again when it is set.
            ("font", Some(v)) => {
                if let Some(&font) = assets.fonts.get(v) {
                    style.font = Some(font);
                    style.bold = false;
                }
            }
            ("size", Some(v)) => match v.parse::<f32>() {
                Ok(size) if size > 0.0 && size.is_finite() => style.size = Some(size),
                _ => return false,
            },
            ("link", Some(v)) if !v.is_empty() => {
                style.link = Some(self.links.len() as u32);
                style.underline = true;
                self.links.push(v.to_string());
            }
            ("img", Some(v)) => {
                let mut alt = None;
                while let Some((key, value)) = next_pair(&mut s) {
                    if key.eq_ignore_ascii_case("alt") {
                        alt = value;
                    }
                }
                let alt = alt.unwrap_or(v);
                match assets.images.get(v) {
                    Some(&texture) => {
                        let start = self.text.len() as u32;
                        self.text.push(OBJECT);
                        self.plain.push_str(alt);
                        self.spans.push(Span {
                            start,
                            end: self.text.len() as u32,
                            style: *style,
                            image: Some(texture),
                            level: 0,
                        });
                    }
                    // No such image (yet): its alternative text stands in.
                    None => self.push_text(alt, *style),
                }
                return true;
            }
            _ => return false,
        }
        open.push((tag, before));
        true
    }

    /// Where lines may end: Unicode's line-break opportunities with
    /// `text-shaping`, else after each run of spaces.
    fn find_breaks(&mut self) {
        let mut start = 0;
        for paragraph in self.text.split('\n') {
            #[cfg(feature = "text-shaping")]
            for (at, _) in unicode_linebreak::linebreaks(paragraph) {
                if at < paragraph.len() {
                    self.breaks.push((start + at) as u32);
                }
            }
            #[cfg(not(feature = "text-shaping"))]
            {
                let mut space = false;
                for (i, c) in paragraph.char_indices() {
                    if space && !c.is_whitespace() {
                        self.breaks.push((start + i) as u32);
                    }
                    space = c.is_whitespace();
                }
            }
            start += paragraph.len() + 1;
        }
    }

    /// The runs' bidirectional embedding levels: spans are split where the
    /// level changes, so each one reads one way.
    fn find_levels(&mut self) {
        #[cfg(feature = "text-shaping")]
        {
            if self.text.is_ascii() {
                return;
            }
            let info = unicode_bidi::BidiInfo::new(&self.text, None);
            if !info.has_rtl() {
                return;
            }
            let mut split = Vec::with_capacity(self.spans.len() + 4);
            for span in &self.spans {
                let mut at = span.start;
                let mut level = info.levels[at as usize].number();
                for (i, _) in self.text[span.start as usize..span.end as usize].char_indices() {
                    let i = span.start + i as u32;
                    let here = info.levels[i as usize].number();
                    if here != level {
                        split.push(Span {
                            start: at,
                            end: i,
                            level,
                            ..*span
                        });
                        at = i;
                        level = here;
                    }
                }
                split.push(Span {
                    start: at,
                    level,
                    ..*span
                });
            }
            self.spans = split;
        }
    }

    /// The font and size a span is measured and drawn in.
    pub(crate) fn font_size(&self, span: &Span, base: Base) -> (Font, f32) {
        let font = if base.fonts {
            span.style.font.unwrap_or(base.font)
        } else {
            base.font
        };
        (font, span.style.size.unwrap_or(base.size) * base.scale)
    }

    /// Ascent and height of span `i`'s line box.
    fn line_box(&self, i: usize, base: Base, text: &mut crate::text::Text) -> (f32, f32) {
        let (font, size) = self.font_size(&self.spans[i], base);
        let (ascent, height) = text.line_box(font, size * base.pixels);
        (ascent / base.pixels, height / base.pixels)
    }

    /// The width of bytes `a..b` of span `i`.
    fn piece_width(&self, i: usize, a: u32, b: u32, base: Base, text: &mut crate::text::Text) -> f32 {
        let span = &self.spans[i];
        let (font, size) = self.font_size(span, base);
        if let Some(image) = span.image {
            return image_size(image, size).x;
        }
        let w = text
            .layout(&self.text[a as usize..b as usize], font, size * base.pixels, false)
            .x
            / base.pixels;
        if span.style.bold { w + bold_offset(size) } else { w }
    }

    /// The width of bytes `a..b`, run by run.
    fn width(&self, a: u32, b: u32, base: Base, text: &mut crate::text::Text) -> f32 {
        let first = self.spans.partition_point(|s| s.end <= a);
        let mut w = 0.0;
        for (i, s) in self.spans.iter().enumerate().skip(first) {
            if s.start >= b {
                break;
            }
            w += self.piece_width(i, s.start.max(a), s.end.min(b), base, text);
        }
        w
    }

    /// `a..b` without trailing whitespace.
    fn trim_end(&self, a: u32, b: u32) -> u32 {
        a + self.text[a as usize..b as usize].trim_end().len() as u32
    }

    /// Each paragraph's byte range.
    fn paragraphs(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let mut start = 0u32;
        self.text.split('\n').map(move |p| {
            let range = (start, start + p.len() as u32);
            start = range.1 + 1;
            range
        })
    }

    /// The widest piece that can't be wrapped: a word, an image.
    pub(crate) fn min_content(&self, base: Base, text: &mut crate::text::Text) -> f32 {
        let mut widest = 0.0f32;
        for (ps, pe) in self.paragraphs() {
            let mut start = ps;
            let first = self.breaks.partition_point(|&b| b <= ps);
            let ends = self.breaks[first..].iter().copied().take_while(|&b| b < pe).chain([pe]);
            for end in ends {
                let trimmed = self.trim_end(start, end);
                widest = widest.max(self.width(start, trimmed, base, text));
                start = end;
            }
        }
        widest
    }

    /// Lays the text out into `out`, wrapping at `wrap` when given (`out`'s
    /// [`room`](RichLayout::room) says which widths wrap it the same way).
    /// Returns its size.
    pub(crate) fn layout(
        &self,
        base: Base,
        wrap: Option<f32>,
        align: Align,
        text: &mut crate::text::Text,
        out: &mut RichLayout,
    ) -> Vec2 {
        out.items.clear();
        let mut lines = Lines {
            rich: self,
            base,
            align,
            text,
            items: &mut out.items,
            room: crate::text::WrapRoom::new(wrap.map_or(0.0, |w| w.max(0.0))),
            y: 0.0,
            widest: 0.0,
        };
        for (ps, pe) in self.paragraphs() {
            lines.paragraph(ps, pe, wrap.is_some());
        }
        let size = Vec2::new(lines.widest, lines.y);
        out.room = lines.room;
        size
    }

    /// The link under `point`, given in the layout's units from the text
    /// box's top left corner, after [`layout`](Self::layout) into `out`.
    pub(crate) fn link_at(&self, out: &RichLayout, point: Vec2) -> Option<u32> {
        out.items.iter().find_map(|item| {
            let inside = point.x >= item.at.x
                && point.x < item.at.x + item.width
                && point.y >= item.top
                && point.y < item.bottom;
            if inside {
                self.spans[item.span as usize].style.link
            } else {
                None
            }
        })
    }
}

/// An image's size on a line of `size`: as tall as the font size, its
/// aspect kept.
pub(crate) fn image_size(image: Texture, size: f32) -> Vec2 {
    let s = image.size();
    Vec2::new(size * s.x / s.y.max(1.0), size)
}

/// How far apart a bold span without a bold font is drawn twice.
pub(crate) fn bold_offset(size: f32) -> f32 {
    (size / 20.0).max(1.0)
}

/// Lines being laid out.
struct Lines<'a> {
    rich: &'a RichText,
    base: Base,
    align: Align,
    text: &'a mut crate::text::Text,
    items: &'a mut Vec<Item>,
    /// The width wrapped at, and the widths that break the lines alike.
    room: crate::text::WrapRoom,
    y: f32,
    widest: f32,
}

impl Lines<'_> {
    /// Lays paragraph `ps..pe` out, wrapped in `self.room` if it `wrap`s:
    /// each decision of where a line ends narrows the room's range.
    fn paragraph(&mut self, ps: u32, pe: u32, wrap: bool) {
        let rich = self.rich;
        let rtl = self.align.directional && crate::text::Text::is_rtl(&rich.text[ps as usize..pe as usize]);
        if !wrap {
            self.line(ps, pe, rtl);
            return;
        }
        let mut line_start = ps;
        let mut line_width = 0.0;
        let mut segment = ps;
        let first = rich.breaks.partition_point(|&b| b <= ps);
        let breaks = rich.breaks[first..].iter().copied().take_while(|&b| b < pe).chain([pe]);
        for end in breaks {
            let trimmed = rich.trim_end(segment, end);
            let fits = rich.width(segment, trimmed, self.base, self.text);
            if line_start < segment && self.room.exceeds(line_width + fits) {
                self.line(line_start, segment, rtl);
                line_start = segment;
                line_width = 0.0;
            }
            if line_start == segment && self.room.exceeds(fits) {
                // A word wider than the line: broken between characters,
                // at least one on each line.
                let mut at = segment;
                loop {
                    let chars = rich.text[at as usize..trimmed as usize].char_indices();
                    let mut cut = at;
                    for (i, c) in chars {
                        let next = at + (i + c.len_utf8()) as u32;
                        if cut > at && self.room.exceeds(rich.width(at, next, self.base, self.text)) {
                            break;
                        }
                        cut = next;
                    }
                    if cut >= trimmed {
                        break;
                    }
                    self.line(at, cut, rtl);
                    at = cut;
                }
                line_start = at;
                line_width = rich.width(at, end, self.base, self.text);
            } else {
                line_width += rich.width(segment, end, self.base, self.text);
            }
            segment = end;
        }
        self.line(line_start, pe, rtl);
    }

    /// Places bytes `a..b` (trailing whitespace left out) as one line.
    fn line(&mut self, a: u32, b: u32, rtl: bool) {
        let rich = self.rich;
        let b = rich.trim_end(a, b);
        let first_item = self.items.len();
        let (mut ascent, mut descent) = (0.0f32, 0.0f32);
        let mut width = 0.0;
        let first = rich.spans.partition_point(|s| s.end <= a);
        for (i, s) in rich.spans.iter().enumerate().skip(first) {
            if s.start >= b {
                break;
            }
            let (start, end) = (s.start.max(a), s.end.min(b));
            let w = rich.piece_width(i, start, end, self.base, self.text);
            let (up, height) = rich.line_box(i, self.base, self.text);
            ascent = ascent.max(up);
            descent = descent.max(height - up);
            width += w;
            self.items.push(Item {
                width: w,
                // The line box's ascent until the line's is known.
                baseline: up,
                start,
                end,
                span: i as u32,
                ..Default::default()
            });
        }
        if self.items.len() == first_item {
            // An empty line is as tall as the node's font.
            let (font, size) = (self.base.font, self.base.size * self.base.scale);
            let (up, height) = self.text.line_box(font, size * self.base.pixels);
            ascent = up / self.base.pixels;
            descent = (height - up) / self.base.pixels;
        }
        let line = &mut self.items[first_item..];
        // Visual order: from the highest level down to the lowest odd one,
        // each sequence of runs at that level or higher is reversed.
        let level_of = |it: &Item| rich.spans[it.span as usize].level;
        let highest = line.iter().map(level_of).max().unwrap_or(0);
        let lowest_odd = line
            .iter()
            .map(level_of)
            .filter(|l| l % 2 == 1)
            .min()
            .unwrap_or(highest + 1);
        let mut level = highest;
        while level >= lowest_odd && level > 0 {
            let mut i = 0;
            while i < line.len() {
                if rich.spans[line[i].span as usize].level >= level {
                    let start = i;
                    while i < line.len() && rich.spans[line[i].span as usize].level >= level {
                        i += 1;
                    }
                    line[start..i].reverse();
                } else {
                    i += 1;
                }
            }
            level -= 1;
        }
        let along = self.align.along(rtl);
        let mut x = 0.0;
        let baseline = self.y + ascent;
        let bottom = baseline + descent;
        for item in line.iter_mut() {
            let span = &rich.spans[item.span as usize];
            let up = item.baseline;
            item.at = Vec2::new(x, baseline - up);
            if let Some(image) = span.image {
                let (_, size) = rich.font_size(span, self.base);
                let size = image_size(image, size);
                // Centered on the line box of its span's font.
                item.image_height = size.y;
                let (_, box_height) = rich.line_box(item.span as usize, self.base, self.text);
                item.at.y = baseline - up + (box_height - size.y) * 0.5;
            }
            item.baseline = baseline;
            item.top = self.y;
            item.bottom = bottom;
            item.line = width;
            item.along = along;
            x += item.width;
            if let Some(box_width) = self.align.width {
                item.at.x += item.line_x(box_width);
            }
        }
        self.widest = self.widest.max(width);
        self.y = bottom;
    }
}

impl Node {
    /// The parsed markup of a rich node.
    pub(super) fn rich(&self) -> Option<&RichText> {
        self.rich.as_deref()
    }
}

impl Ui {
    /// Reads the node's text as rich-text markup, or, with `false`, as
    /// plain text again. Rich text is for labels, buttons and toggles; a text
    /// input stays plain (it edits its text as it is). The markup:
    ///
    /// | Markup | Shows |
    /// |---|---|
    /// | `[color=#ffcc00]…[/color]` | the text in a color: `#rgb`, `#rrggbb`, `#rrggbbaa`, or `white`, `black`, `gray`, `red`, `green`, `blue`, `yellow` |
    /// | `[b]…[/b]` | bold: the font named `bold` ([`Ui::set_inline_font`]), else the node's font drawn heavier |
    /// | `[font=name]…[/font]` | the font set under `name` |
    /// | `[size=24]…[/size]` | a font size in UI units |
    /// | `[u]…[/u]` | underlined |
    /// | `[link=target]…[/link]` | a clickable range, underlined: a click sends [`UiEvent::LinkClicked`] |
    /// | `[img=name]`, `[img=name alt="A"]` | the image set under `name` ([`Ui::set_inline_image`]), as tall as the text; a screen reader reads `alt` (else the name) |
    /// | `[[` | a `[` |
    ///
    /// Tags nest and close with `[/name]`; a tag left open runs to the end.
    /// Anything in brackets that isn't one of these (`[A]`, a bad color)
    /// is shown as it is. Spans wrap with the rest of the text, and with
    /// `text-shaping` each run is shaped and right-to-left runs are put in
    /// visual order (a word split across spans is shaped in pieces).
    /// [`Ui::accessible_name`] is the text without its markup.
    ///
    /// Keyed text ([`Ui::set_text_key`]) can be rich: the string table entry
    /// holds the markup, and text arguments are escaped before they are
    /// put in, so a player's name can't add markup.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiLayout, UiWidget};
    ///
    /// let mut ui = Ui::default();
    /// let line = ui.add(UiWidget::Label, UiLayout::auto());
    /// ui.set_text(line, "[color=#ffcc00]Mira:[/color] Press [img=pad_a alt=A] to talk.");
    /// ui.set_rich(line, true);
    /// assert_eq!(ui.accessible_name(line), Some("Mira: Press A to talk."));
    /// ```
    pub fn set_rich(&mut self, id: UiId, on: bool) {
        let Some(n) = self.node_mut(id) else { return };
        if on == n.rich.is_some() || n.widget == UiWidget::TextInput {
            return;
        }
        n.rich = on.then(Box::default);
        if on && n.key.is_some() {
            // Its arguments are escaped from now on.
            self.locale.dirty = true;
        }
        self.parse_rich_node(id);
    }

    /// Whether the node's text is read as markup ([`Ui::set_rich`]).
    ///
    /// ```
    /// use ponko::ui::{Ui, UiLayout, UiWidget};
    /// let mut ui = Ui::default();
    /// let label = ui.add(UiWidget::Label, UiLayout::auto());
    /// assert!(!ui.is_rich(label));
    /// ui.set_rich(label, true);
    /// assert!(ui.is_rich(label));
    /// ```
    #[must_use]
    pub fn is_rich(&self, id: UiId) -> bool {
        self.node(id).is_some_and(|n| n.rich.is_some())
    }

    /// Names `texture` for rich text's `[img=name]`: a button glyph, an
    /// icon. Nodes that show the name are updated.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// let pad_a = ctx.gfx.load_texture("ui/pad_a.png");
    /// ctx.ui.set_inline_image("pad_a", pad_a);
    /// # }
    /// ```
    pub fn set_inline_image(&mut self, name: &str, texture: Texture) {
        self.rich_assets.set_image(name, texture);
        self.parse_rich();
    }

    /// Names `font` for rich text's `[font=name]`; the name `bold` is the
    /// font `[b]` uses. Nodes that use the name are updated.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// let bold = ctx.gfx.load_font("fonts/Bold.ttf");
    /// ctx.ui.set_inline_font("bold", bold);
    /// # }
    /// ```
    pub fn set_inline_font(&mut self, name: &str, font: Font) {
        self.rich_assets.set_font(name, font);
        self.parse_rich();
    }

    /// The target of link `index` of a rich node (`[link=target]`, counted
    /// from 0 in the order they appear), as [`UiEvent::LinkClicked`] names
    /// it.
    ///
    /// ```
    /// use ponko::ui::{Ui, UiLayout, UiWidget};
    /// let mut ui = Ui::default();
    /// let label = ui.add(UiWidget::Label, UiLayout::auto());
    /// ui.set_text(label, "Read the [link=rules]rules[/link].");
    /// ui.set_rich(label, true);
    /// ui.layout(glam::vec2(320.0, 240.0));
    /// assert_eq!(ui.link(label, 0), Some("rules"));
    /// assert_eq!(ui.link(label, 1), None);
    /// ```
    #[must_use]
    pub fn link(&self, id: UiId, index: u32) -> Option<&str> {
        self.node(id)?.rich()?.links.get(index as usize).map(String::as_str)
    }

    /// Parses node `id`'s markup if its text or the names changed.
    pub(super) fn parse_rich_node(&mut self, id: UiId) {
        let assets = &self.rich_assets;
        if let Some(Some(n)) = self
            .nodes
            .get_mut(id.slot())
            .filter(|n| n.as_ref().is_some_and(|n| n.id == id))
            && let Some(rich) = n.rich.as_deref_mut()
            && rich.parsed != assets.revision
        {
            rich.parse(&n.text, assets);
        }
    }

    /// Parses the markup of rich nodes whose text or names changed.
    pub(super) fn parse_rich(&mut self) {
        let assets = &self.rich_assets;
        for n in self.nodes.iter_mut().flatten() {
            if let Some(rich) = n.rich.as_deref_mut()
                && rich.parsed != assets.revision
            {
                rich.parse(&n.text, assets);
            }
        }
    }

    /// The box a node's text is laid out in.
    pub(super) fn text_box(&self, id: UiId) -> Option<Rect> {
        let n = self.node(id)?;
        let layout = self.tree.layout(n.layout).ok()?;
        let scale = self.scale;
        let mut min = n.rect.min
            + Vec2::new(
                layout.padding.left + layout.border.left,
                layout.padding.top + layout.border.top,
            ) * scale;
        let max = n.rect.max
            - Vec2::new(
                layout.padding.right + layout.border.right,
                layout.padding.bottom + layout.border.bottom,
            ) * scale;
        if n.widget == UiWidget::Toggle {
            min.x += (max.y - min.y).min(20.0 * scale) + 6.0 * scale;
        }
        Some(Rect { min, max })
    }

    /// The link of rich node `id` under `point` (window units), if any.
    pub(super) fn link_under(&mut self, id: UiId, point: Vec2) -> Option<u32> {
        let rect = self.text_box(id)?;
        // The node from its field, so the measure and scratch borrow apart.
        let n = self.nodes.get(id.slot())?.as_ref().filter(|n| n.id == id)?;
        let rich = n.rich()?;
        if rich.links.is_empty() {
            return None;
        }
        let base = Base {
            font: if self.external_fonts {
                n.paint.font
            } else {
                Font::DEFAULT
            },
            size: n.paint.font_size,
            scale: self.scale,
            fonts: self.external_fonts,
            pixels: self.pixels,
        };
        let align = Align {
            align: n.paint.align,
            width: Some(rect.size().x),
            directional: true,
        };
        let wrap = n.paint.wrap.then_some(rect.size().x);
        rich.layout(base, wrap, align, &mut self.measure, &mut self.rich_layout);
        rich.link_at(&self.rich_layout, point - rect.min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(markup: &str, assets: &RichAssets) -> RichText {
        let mut rich = RichText::default();
        rich.parse(markup, assets);
        rich
    }

    #[test]
    fn tags_become_spans_over_the_text_without_them() {
        let rich = parse(
            "[color=#ff0000]Mira:[/color] [size=30]hi[/size] [u]you[/u]",
            &RichAssets::default(),
        );
        assert_eq!(rich.text, "Mira: hi you");
        let runs: Vec<(&str, Style)> = rich
            .spans
            .iter()
            .map(|s| (&rich.text[s.start as usize..s.end as usize], s.style))
            .collect();
        assert_eq!(
            runs[0],
            (
                "Mira:",
                Style {
                    color: Some(Color::RED),
                    ..Default::default()
                }
            )
        );
        assert_eq!(runs[1], (" ", Style::default()));
        assert_eq!(
            runs[2],
            (
                "hi",
                Style {
                    size: Some(30.0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            runs[4],
            (
                "you",
                Style {
                    underline: true,
                    ..Default::default()
                }
            )
        );
        assert_eq!(runs.len(), 5);
    }

    #[test]
    fn nested_tags_close_back_to_the_style_around_them() {
        let rich = parse("[color=red]a[b]b[/b]c[/color]d", &RichAssets::default());
        let styles: Vec<Style> = rich.spans.iter().map(|s| s.style).collect();
        let red = Style {
            color: Some(Color::RED),
            ..Default::default()
        };
        assert_eq!(styles, [red, Style { bold: true, ..red }, red, Style::default()]);
    }

    #[test]
    fn what_is_not_markup_is_shown_as_it_is() {
        let rich = parse(
            "Press [A], [color=nope]x[/color], [/b] and [[b]] [b",
            &RichAssets::default(),
        );
        assert_eq!(rich.text, "Press [A], [color=nope]x[/color], [/b] and [b]] [b");
        assert_eq!(rich.spans.len(), 1);
    }

    #[test]
    fn colors_are_read_in_three_six_and_eight_digits() {
        assert_eq!(parse_color("#fff"), Some(Color::WHITE));
        assert_eq!(parse_color("#ffcc00"), Some(Color::hex(0xffcc00)));
        assert_eq!(parse_color("#ff000080"), Some(Color::RED.with_alpha(128.0 / 255.0)));
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("#ggg"), None);
    }

    #[test]
    fn images_stand_in_the_text_and_read_as_their_alternative() {
        let mut assets = RichAssets::default();
        assets.set_image("pad_a", Texture::new(5, 32, 16));
        let rich = parse("Press [img=pad_a alt=\"button A\"] or [img=pad_b].", &assets);
        assert_eq!(rich.text, "Press \u{FFFC} or pad_b.");
        assert_eq!(rich.plain, "Press button A or pad_b.");
        let image = rich.spans.iter().find(|s| s.image.is_some()).unwrap();
        assert_eq!(&rich.text[image.start as usize..image.end as usize], "\u{FFFC}");
        assert_eq!(image_size(image.image.unwrap(), 20.0), Vec2::new(40.0, 20.0));
    }

    #[test]
    fn bold_uses_the_font_named_bold_when_there_is_one() {
        let mut assets = RichAssets::default();
        assert!(parse("[b]x", &assets).spans[0].style.bold);
        assets.set_font("bold", Font(3));
        let style = parse("[b]x", &assets).spans[0].style;
        assert_eq!((style.bold, style.font), (false, Some(Font(3))));
    }

    #[test]
    fn links_are_numbered_in_order_and_underlined() {
        let rich = parse("[link=a]one[/link] [link=b]two[/link]", &RichAssets::default());
        assert_eq!(rich.links, ["a", "b"]);
        let links: Vec<Option<u32>> = rich.spans.iter().map(|s| s.style.link).collect();
        assert_eq!(links, [Some(0), None, Some(1)]);
        assert!(rich.spans[0].style.underline);
    }

    #[test]
    fn escaped_text_reads_back_as_itself() {
        let mut escaped = String::new();
        escape_into("[b]Ann[/b] [[", &mut escaped);
        assert_eq!(parse(&escaped, &RichAssets::default()).text, "[b]Ann[/b] [[");
    }

    #[test]
    fn lines_break_between_words() {
        let rich = parse("one two three", &RichAssets::default());
        assert_eq!(rich.breaks, [4, 8]);
    }

    #[cfg(feature = "text-shaping")]
    #[test]
    fn right_to_left_runs_get_odd_levels() {
        let rich = parse("[color=red]abc مرحبا[/color] بالعالم", &RichAssets::default());
        let levels: Vec<(&str, u8)> = rich
            .spans
            .iter()
            .map(|s| (&rich.text[s.start as usize..s.end as usize], s.level))
            .collect();
        assert_eq!(levels[0], ("abc ", 0));
        assert_eq!(levels[1], ("مرحبا", 1));
        assert_eq!(levels.last().unwrap().1, 1);
    }

    #[test]
    fn a_wrapped_line_is_placed_line_by_line_and_aligned() {
        let rich = parse("aaa bbb ccc", &RichAssets::default());
        let mut text = crate::text::Text::new();
        let base = Base {
            font: Font::DEFAULT,
            size: 16.0,
            scale: 1.0,
            fonts: true,
            pixels: 1.0,
        };
        let mut out = RichLayout::default();
        let one = rich.layout(base, None, Align::MEASURE, &mut text, &mut out);
        assert_eq!(out.items.len(), 1);
        let word = text.layout("aaa bbb", Font::DEFAULT, 16.0, false).x;
        let size = rich.layout(base, Some(word + 1.0), Align::MEASURE, &mut text, &mut out);
        assert_eq!(out.items.len(), 2, "two lines");
        assert!(size.y > one.y * 1.9 && size.x < one.x);
        assert_eq!(
            &rich.text[out.items[0].start as usize..out.items[0].end as usize],
            "aaa bbb"
        );
        assert_eq!(
            &rich.text[out.items[1].start as usize..out.items[1].end as usize],
            "ccc"
        );
        // Right-aligned lines end at the box's right edge.
        let align = Align {
            align: UiTextAlign::End,
            width: Some(200.0),
            directional: false,
        };
        rich.layout(base, Some(200.0), align, &mut text, &mut out);
        let last = out.items[0];
        assert!((last.at.x + last.width - 200.0).abs() < 0.01);
        assert!(rich.min_content(base, &mut text) < one.x / 2.0);
    }
}
