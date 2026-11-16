//! Rich text in retained UI labels (`Ui::set_rich`): markup, wrapping,
//! inline images, links, screen-reader text and keyed, localized text.
//! Headless: the UI measures text with the built-in font.

use ponko::locale::{Arg, StringTable, Strings};
use ponko::prelude::*;
use ponko::ui::{Ui, UiEvent, UiId, UiInput, UiLayout, UiWidget};

const VIEW: Vec2 = Vec2::new(400.0, 300.0);

/// The whole window, its children as wide as their text (not stretched).
fn root_layout() -> UiLayout {
    let mut layout = UiLayout::fill();
    layout.unstable_style_mut().align_items = ponko::ui::unstable_taffy::AlignItems::START;
    layout
}

/// A screen with one label of `text`, `width` wide when given.
fn screen(text: &str, width: Option<f32>, rich: bool) -> (Ui, UiId) {
    let mut ui = Ui::default();
    let root = ui.root().layout(root_layout()).id();
    let layout = width.map_or(UiLayout::auto(), |w| UiLayout::auto().width(w));
    let label = ui.label(root, text).layout(layout).wrap().id();
    ui.set_rich(label, rich);
    ui.push_modeless(root);
    ui.layout(VIEW);
    (ui, label)
}

/// Pressed and released at `at`: the events of the release.
fn click(ui: &mut Ui, at: Vec2) -> Vec<UiEvent> {
    let press = UiInput {
        pointer: at,
        pressed: true,
        held: true,
        ..Default::default()
    };
    ui.frame(VIEW, &press);
    let release = UiInput {
        pointer: at,
        released: true,
        ..Default::default()
    };
    ui.frame(VIEW, &release);
    ui.events().collect()
}

#[test]
fn markup_is_not_shown_or_read() {
    let (ui, label) = screen("[color=#ffcc00]Mira:[/color] [b]Hello[/b] [[friend]", None, true);
    assert_eq!(ui.accessible_name(label), Some("Mira: Hello [friend]"));
    // The markup itself stays the node's text.
    assert_eq!(
        ui.text(label),
        Some("[color=#ffcc00]Mira:[/color] [b]Hello[/b] [[friend]")
    );
    let (plain, plain_label) = screen("Mira: Hello [friend]", None, false);
    let (w, plain_w) = (
        ui.rect(label).unwrap().size().x,
        plain.rect(plain_label).unwrap().size().x,
    );
    assert!(
        (w - plain_w).abs() < 4.0,
        "as wide as the same text without markup (bold a little wider): {w} vs {plain_w}"
    );
}

#[test]
fn rich_text_wraps_like_plain_text() {
    let line = "Press the button to talk to the keeper before the bell rings at dusk.";
    let (plain, plain_label) = screen(line, Some(150.0), false);
    let (rich, label) = screen(&format!("[color=#ffcc00]{line}[/color]"), Some(150.0), true);
    let (plain_h, rich_h) = (
        plain.rect(plain_label).unwrap().size().y,
        rich.rect(label).unwrap().size().y,
    );
    let (one, one_label) = screen("Press", None, true);
    let line_height = one.rect(one_label).unwrap().size().y;
    assert!(rich_h > 3.0 * line_height, "wraps onto four lines or more: {rich_h}");
    assert!(
        rich_h >= plain_h - 0.5 && rich_h <= plain_h + 0.5,
        "{rich_h} vs {plain_h}"
    );
}

#[test]
fn a_bigger_span_makes_its_line_taller() {
    let (ui, label) = screen("Level [size=36]3[/size]", None, true);
    let (small, small_label) = screen("Level 3", None, true);
    assert!(ui.rect(label).unwrap().size().y > small.rect(small_label).unwrap().size().y * 1.5);
}

#[test]
fn inline_images_take_room_as_tall_as_the_text() {
    Game::new("rich", 320, 240).run_headless(1, |ctx| {
        let glyph = ctx.gfx.create_texture(32, 16, &[255; 32 * 16 * 4]);
        ctx.ui.set_inline_image("pad_a", glyph);
        let root = ctx.ui.root().layout(root_layout()).id();
        let with = ctx.ui.label(root, "Press [img=pad_a alt=A] now").rich().id();
        let without = ctx.ui.label(root, "Press  now").rich().id();
        ctx.ui.push_modeless(root);
        ctx.ui.layout(VIEW);
        let extra = ctx.ui.rect(with).unwrap().size().x - ctx.ui.rect(without).unwrap().size().x;
        // Twice as wide as tall, at the label's 18 unit text (its theme size).
        let size = ctx.ui.paint(with).unwrap().font_size;
        assert!((extra - 2.0 * size).abs() < 0.5, "{extra} for {size} unit text");
        assert_eq!(ctx.ui.accessible_name(with), Some("Press A now"));
    });
}

#[test]
fn an_image_named_later_replaces_its_alternative_text() {
    Game::new("rich", 320, 240).run_headless(1, |ctx| {
        let root = ctx.ui.root().layout(root_layout()).id();
        let label = ctx.ui.label(root, "[img=pad_a alt=AAAAAAAA]").rich().id();
        ctx.ui.push_modeless(root);
        ctx.ui.layout(VIEW);
        let before = ctx.ui.rect(label).unwrap().size().x;
        let glyph = ctx.gfx.create_texture(16, 16, &[255; 16 * 16 * 4]);
        ctx.ui.set_inline_image("pad_a", glyph);
        ctx.ui.layout(VIEW);
        let after = ctx.ui.rect(label).unwrap().size().x;
        assert!(after < before / 2.0, "the image, not the text: {after} vs {before}");
    });
}

#[test]
fn clicking_a_link_sends_its_index_and_elsewhere_nothing() {
    let (mut ui, label) = screen(
        "See [link=rules]the rules[/link] or [link=map]the map[/link].",
        None,
        true,
    );
    let rect = ui.rect(label).unwrap();
    let mid = rect.center().y;
    // "See " is not a link.
    assert!(click(&mut ui, Vec2::new(rect.min.x + 3.0, mid)).is_empty());
    // The far end of the line is in "the map", the second link.
    let events = click(&mut ui, Vec2::new(rect.max.x - 12.0, mid));
    assert_eq!(events, [UiEvent::LinkClicked(label, 1)]);
    assert_eq!(ui.link(label, 1), Some("map"));
    // Just after "See " is the first.
    let see = ui.rect(label).unwrap().min.x + 40.0;
    assert_eq!(click(&mut ui, Vec2::new(see, mid)), [UiEvent::LinkClicked(label, 0)]);
}

#[test]
fn a_link_in_a_button_takes_the_click_and_the_rest_clicks_the_button() {
    let mut ui = Ui::default();
    let root = ui.root().layout(root_layout()).id();
    let button = ui.button(root, "Agree to the [link=terms]terms[/link]").rich().id();
    ui.push_modeless(root);
    ui.layout(VIEW);
    let rect = ui.rect(button).unwrap();
    let left = Vec2::new(rect.min.x + 14.0, rect.center().y);
    assert_eq!(click(&mut ui, left), [UiEvent::Clicked(button)]);
    // The link ends the text: just inside the right padding.
    let events = click(&mut ui, Vec2::new(rect.max.x - 16.0, rect.center().y));
    assert_eq!(events, [UiEvent::LinkClicked(button, 0)]);
}

#[test]
fn plain_text_again_shows_the_markup() {
    let (mut ui, label) = screen("[b]Hi[/b]", None, true);
    assert!(ui.is_rich(label));
    ui.set_rich(label, false);
    assert_eq!(ui.accessible_name(label), Some("[b]Hi[/b]"));
    assert_eq!(ui.link(label, 0), None);
}

#[test]
fn text_inputs_stay_plain() {
    let mut ui = Ui::default();
    let input = ui.add(UiWidget::TextInput, UiLayout::auto());
    ui.set_rich(input, true);
    assert!(!ui.is_rich(input));
}

#[test]
fn keyed_text_is_rich_in_every_language_and_arguments_stay_text() {
    let mut strings = Strings::default();
    strings.insert(
        StringTable::from_json(
            r#"{"language": "en", "strings": {"talk": "[color=#ffcc00]{name}:[/color] Press [img=pad_a alt=A] to talk."}}"#,
        )
        .unwrap(),
    );
    strings.insert(
        StringTable::from_json(
            r#"{"language": "it", "strings": {"talk": "[color=#ffcc00]{name}:[/color] Premi [img=pad_a alt=A] per parlare."}}"#,
        )
        .unwrap(),
    );
    let mut ui = Ui::default();
    let root = ui.root().id();
    let line = ui.label(root, "").rich().id();
    // A player's name with brackets is shown as it is, not read as markup.
    ui.set_text_key(line, "talk", &[("name", Arg::from("[b]Ann"))]);
    ui.localize(&strings);
    assert_eq!(ui.accessible_name(line), Some("[b]Ann: Press A to talk."));
    strings.set_language("it");
    ui.localize(&strings);
    assert_eq!(ui.accessible_name(line), Some("[b]Ann: Premi A per parlare."));
}

#[test]
fn turning_rich_on_after_the_key_escapes_its_arguments() {
    let mut strings = Strings::default();
    strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"hi": "[u]Hi[/u] {name}"}}"#).unwrap());
    let mut ui = Ui::default();
    let label = ui.add(UiWidget::Label, UiLayout::auto());
    ui.set_text_key(label, "hi", &[("name", Arg::from("[u]x"))]);
    ui.localize(&strings);
    assert_eq!(ui.accessible_name(label), Some("[u]Hi[/u] [u]x"));
    ui.set_rich(label, true);
    ui.localize(&strings);
    assert_eq!(ui.accessible_name(label), Some("Hi [u]x"));
}

#[cfg(feature = "text-shaping")]
#[test]
fn a_right_to_left_line_puts_its_first_run_at_the_right() {
    // "Hello world": the first word is a link. Read right to left, it is at
    // the right; the label is wider than its text, so the line ends there.
    let (mut ui, label) = screen("[link=a]مرحبا[/link] بالعالم بالعالم", Some(380.0), true);
    let rect = ui.rect(label).unwrap();
    let mid = rect.center().y;
    assert_eq!(
        click(&mut ui, Vec2::new(rect.max.x - 6.0, mid)),
        [UiEvent::LinkClicked(label, 0)]
    );
    assert!(click(&mut ui, Vec2::new(rect.min.x + 6.0, mid)).is_empty());
}

#[cfg(feature = "text-shaping")]
#[test]
fn a_right_to_left_string_table_entry_keeps_its_markup_and_order() {
    let mut strings = Strings::default();
    strings.insert(
        StringTable::from_json(r#"{"language": "en", "strings": {"greet": "[link=who]{name}[/link]: hello"}}"#)
            .unwrap(),
    );
    strings.insert(
        StringTable::from_json(r#"{"language": "ar", "strings": {"greet": "[link=who]{name}[/link]: مرحبا بالعالم"}}"#)
            .unwrap(),
    );
    strings.set_language("ar");
    let mut ui = Ui::default();
    let root = ui.root().layout(root_layout()).id();
    let line = ui.label(root, "").rich().layout(UiLayout::auto().width(380.0)).id();
    ui.set_text_key(line, "greet", &[("name", Arg::from("مريم"))]);
    ui.push_modeless(root);
    ui.localize(&strings);
    ui.layout(VIEW);
    assert_eq!(ui.accessible_name(line), Some("مريم: مرحبا بالعالم"));
    let rect = ui.rect(line).unwrap();
    // The name comes first, so it is at the right of a right-to-left line.
    let mid = rect.center().y;
    assert_eq!(
        click(&mut ui, Vec2::new(rect.max.x - 6.0, mid)),
        [UiEvent::LinkClicked(line, 0)]
    );
    // English, with a name in Latin letters: left to right.
    strings.set_language("en");
    ui.set_text_key(line, "greet", &[("name", Arg::from("Mira"))]);
    ui.localize(&strings);
    ui.layout(VIEW);
    assert_eq!(click(&mut ui, Vec2::new(rect.max.x - 6.0, mid)), []);
    assert_eq!(
        click(&mut ui, Vec2::new(rect.min.x + 6.0, mid)),
        [UiEvent::LinkClicked(line, 0)]
    );
}
