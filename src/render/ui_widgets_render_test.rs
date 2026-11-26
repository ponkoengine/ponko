//! The widget set drawn offscreen, state by state, at UI scale 1 (text at
//! the size a game shows it):
//! - the controls (dropdown, number field, a key-binding field listening, a
//!   button, tabs) and the choices (radio buttons, a checkbox, a spinner, a
//!   progress bar, split panes), each in the dark and the light theme;
//! - the controls with the dropdown open, the choices with a context menu
//!   open and with a tooltip shown;
//! - a list, a grid and an item dragged onto a container.
//!
//! The engine's UI drawing makes the sprites; the window's sprite shader,
//! pipeline and pass draw them into a texture read back here (the rig of
//! `ui_render_test`). Each picture is checked against its reference image
//! with a negative control, after assertions on what it must show: the
//! states, text centred in its control and clear of its edges, and text at
//! least 4.5:1 against its control's fill as drawn. Skips without a GPU (a
//! skip is not a pass).
use super::render_test_support::{Image, Tolerance, check_reference, keep_image, reference_rejects};
use super::ui_render_test::{Painter, Screen};
use crate::camera::Rect;
use crate::ui::{UiAccessAction, UiAxis, UiId, UiInput, UiLayout, UiTheme};
use glam::{Vec2, vec2};

/// Text is rasterized on the CPU and sampled texel for texel, and round
/// corners and radio buttons sample one disc texture: adapters differ only
/// in the last bits of blending and filtering.
const REFERENCE: Tolerance = Tolerance {
    channel: 24,
    outliers: 0.004,
    edge: 1,
};

/// WCAG's minimum contrast for body text, checked on the drawn pixels.
const TEXT_CONTRAST: f32 = 4.5;

/// The windows (references stay at most 256 pixels on a side).
const CONTROLS: Vec2 = Vec2::new(256.0, 168.0);
const CHOICES: Vec2 = Vec2::new(256.0, 160.0);
const INVENTORY: Vec2 = Vec2::new(256.0, 200.0);

const PIPELINE: &str = "UI scale 1, drawn with the window's sprite pipeline into an sRGB target";

/// The controls sheet's nodes.
struct Controls {
    dropdown: UiId,
    number: UiId,
    binding: UiId,
    apply: UiId,
    tabs: UiId,
}

/// The choices sheet's nodes.
struct Choices {
    radios: [UiId; 3],
    music: UiId,
    progress: UiId,
    panes: (UiId, UiId),
}

fn root(screen: &mut Screen, theme: UiTheme) -> UiId {
    let ui = &mut screen.ctx.ui;
    ui.set_theme(theme);
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(8.0)).id();
    ui.push_modeless(root);
    root
}

/// A dropdown, a number field, a key-binding field (listening with
/// `listen`), a button and a tab strip with its second tab chosen.
fn controls(screen: &mut Screen, theme: UiTheme, listen: bool) -> Controls {
    let root = root(screen, theme);
    let ui = &mut screen.ctx.ui;
    let first = ui.panel(root).layout(UiLayout::row(32.0).gap(8.0)).id();
    let dropdown = ui
        .dropdown(first, ["Low", "Medium", "High"])
        .selected(2)
        .layout(UiLayout::fixed(120.0, 32.0))
        .id();
    let number = ui
        .number_field(first, 5.0)
        .range(0.0, 10.0)
        .layout(UiLayout::fixed(96.0, 32.0))
        .id();
    let second = ui.panel(root).layout(UiLayout::row(32.0).gap(8.0)).id();
    let binding = ui
        .key_binding(
            second,
            Some(crate::actions::ButtonBinding::Key(crate::input::Key::Space)),
        )
        .layout(UiLayout::fixed(140.0, 32.0))
        .id();
    let apply = ui.button(second, "Apply").layout(UiLayout::fixed(80.0, 32.0)).id();
    let tabs = ui.tabs(root).layout(UiLayout::row(64.0)).id();
    for title in ["General", "Audio", "Video"] {
        let page = ui.tab(tabs, title).id();
        ui.label(page, format!("{title} settings"));
    }
    ui.set_selected(tabs, Some(1));
    if listen {
        ui.push_access_action(UiAccessAction::Activate(binding));
    }
    Controls {
        dropdown,
        number,
        binding,
        apply,
        tabs,
    }
}

/// Three radio buttons with the second checked, a checked checkbox with a
/// context menu and a tooltip, a spinner, a progress bar at 60 % and split
/// panes at 0.5.
fn choices(screen: &mut Screen, theme: UiTheme) -> Choices {
    let root = root(screen, theme);
    let ui = &mut screen.ctx.ui;
    let group = ui.radio_group(root).layout(UiLayout::row(28.0).gap(6.0)).id();
    let radios = ["Off", "Low", "High"].map(|t| ui.radio(group, t).layout(UiLayout::fixed(74.0, 28.0)).id());
    ui.set_selected(group, Some(1));
    let second = ui.panel(root).layout(UiLayout::row(28.0).gap(12.0)).id();
    let music = ui
        .toggle(second, "Music")
        .value(1.0)
        .tooltip("Plays the soundtrack")
        .context_menu(["Mute", "Solo"])
        .layout(UiLayout::fixed(110.0, 28.0))
        .id();
    ui.spinner(second);
    let progress = ui.progress_bar(root).value(0.6).layout(UiLayout::row(12.0)).id();
    let split = ui.split(root, UiAxis::Horizontal).layout(UiLayout::row(44.0)).id();
    let panes = ui.panes(split).unwrap();
    // The labels keep clear of the divider.
    ui.label(panes.0, "Left").layout(UiLayout::fill().padding(6.0));
    ui.label(panes.1, "Right").layout(UiLayout::fill().padding(6.0));
    Choices {
        radios,
        music,
        progress,
        panes,
    }
}

/// A frame with the pointer at `at` (off the screen: nothing hovered).
fn frame(screen: &mut Screen, size: Vec2, at: Vec2, held: bool, dt: f32) {
    screen.ctx.ui.frame(
        size,
        &UiInput {
            pointer: at,
            held,
            dt,
            ..Default::default()
        },
    );
}

fn draw(painter: &Painter, screen: &mut Screen, size: Vec2) -> Image {
    screen.ctx.draw_retained_ui();
    let image = painter.shot(screen, size);
    keep_image(&image);
    image
}

fn at(image: &Image, p: Vec2) -> [u8; 4] {
    image.pixel(p.x as u32, p.y as u32)
}

fn far(a: [u8; 4], b: [u8; 4]) -> bool {
    (0..3).any(|c| a[c].abs_diff(b[c]) > 60)
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    (0..3).all(|c| a[c].abs_diff(b[c]) <= 12)
}

/// WCAG contrast of two drawn (sRGB) pixels.
fn contrast(a: [u8; 4], b: [u8; 4]) -> f32 {
    let luminance = |p: [u8; 4]| {
        let l = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(p[0]) + 0.7152 * l(p[1]) + 0.0722 * l(p[2])
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

const OFF: Vec2 = Vec2::new(-10.0, -10.0);

/// The pixels of `area` that differ from `fill`: the text and whatever
/// else is drawn on it.
fn ink(image: &Image, area: Rect, fill: [u8; 4]) -> Vec<(u32, u32, [u8; 4])> {
    let mut out = Vec::new();
    for y in area.min.y.ceil() as u32..area.max.y.floor() as u32 {
        for x in area.min.x.ceil() as u32..area.max.x.floor() as u32 {
            let p = image.pixel(x, y);
            if far(p, fill) {
                out.push((x, y, p));
            }
        }
    }
    out
}

/// Checks a control's text as drawn: its strongest pixel stands out at
/// least 4.5:1 from the fill, it is centred on the control's height (within
/// two pixels) and at least `gap` pixels from the left and right edges.
/// `text` is the part of the control the text goes in (after an arrow, a
/// circle, a box).
fn check_text(image: &Image, what: &str, rect: Rect, text: Rect, fill_at: Vec2, gap: f32) {
    let fill = at(image, fill_at);
    let pixels = ink(image, text, fill);
    assert!(!pixels.is_empty(), "{what}: no text drawn");
    let best = pixels.iter().map(|&(_, _, p)| contrast(p, fill)).fold(0.0, f32::max);
    assert!(
        best >= TEXT_CONTRAST,
        "{what}: text {best:.2}:1 against its fill {fill:?}"
    );
    let (top, bottom) = pixels
        .iter()
        .fold((u32::MAX, 0), |(t, b), &(_, y, _)| (t.min(y), b.max(y)));
    let (left, right) = pixels
        .iter()
        .fold((u32::MAX, 0), |(l, r), &(x, _, _)| (l.min(x), r.max(x)));
    let middle = (top + bottom + 1) as f32 * 0.5;
    assert!(
        (middle - rect.center().y).abs() <= 2.0,
        "{what}: text rows {top}..={bottom}, control middle {}",
        rect.center().y
    );
    assert!(
        left as f32 >= rect.min.x + gap && right as f32 + 1.0 <= rect.max.x - gap,
        "{what}: text columns {left}..={right} within {gap} px of {rect:?}"
    );
}

/// The controls sheet in `theme` (the key-binding field listening) with its
/// checks: the chosen tab underlined in the accent, the button's and the
/// number field's text centred across, every control's text centred, clear
/// of its edges and 4.5:1 against its fill.
fn checked_controls(painter: &Painter, theme: UiTheme) -> Image {
    let mut screen = painter.screen(CONTROLS);
    let s = controls(&mut screen, theme, true);
    frame(&mut screen, CONTROLS, OFF, false, 0.0);
    assert!(screen.ctx.ui.is_capturing(s.binding));
    let image = draw(painter, &mut screen, CONTROLS);
    let ui = &screen.ctx.ui;
    let rect = |id| ui.rect(id).unwrap();
    let audio = rect(ui.tab_button(s.tabs, 1).unwrap());
    let general = rect(ui.tab_button(s.tabs, 0).unwrap());
    let accent = at(&image, vec2(audio.center().x, audio.max.y - 1.5));
    assert!(
        far(accent, at(&image, vec2(general.center().x, general.max.y - 1.5))),
        "one tab underlined"
    );
    // Inside a control, a column clear of its border and its text shows
    // the fill.
    let inner = |r: Rect| vec2(r.min.x + 3.0, r.center().y);
    let gap = theme.spacing * 0.75;
    let d = rect(s.dropdown);
    let arrow = 16.0 + gap;
    check_text(
        &image,
        "dropdown",
        d,
        Rect::new(d.min.x + 3.0, d.min.y + 3.0, d.size().x - arrow - 3.0, d.size().y - 6.0),
        inner(d),
        gap,
    );
    let number = rect(s.number);
    let quarter = number.size().x * 0.25;
    check_text(
        &image,
        "number field",
        number,
        Rect::new(
            number.min.x + quarter,
            number.min.y + 3.0,
            quarter * 2.0,
            number.size().y - 6.0,
        ),
        vec2(number.center().x, number.min.y + 4.0),
        gap,
    );
    for (what, id) in [("key binding", s.binding), ("button", s.apply)] {
        let r = rect(id);
        check_text(&image, what, r, r.expand(-3.0), inner(r), gap);
    }
    // The button's and the listening field's text are centred across.
    for (what, id) in [("button", s.apply), ("key binding", s.binding)] {
        let r = rect(id);
        let pixels = ink(&image, r.expand(-3.0), at(&image, inner(r)));
        let (l, rr) = pixels
            .iter()
            .fold((u32::MAX, 0), |(l, h), &(x, _, _)| (l.min(x), h.max(x)));
        let middle = (l + rr + 1) as f32 * 0.5;
        assert!((middle - r.center().x).abs() <= 2.0, "{what}: text {l}..={rr} in {r:?}");
    }
    for tab in [general, audio] {
        check_text(
            &image,
            "tab",
            tab,
            // Clear of the round corners and the underline.
            Rect::new(tab.min.x + 3.0, tab.min.y + 3.0, tab.size().x - 6.0, tab.size().y - 7.0),
            vec2(tab.min.x + 2.0, tab.center().y),
            gap,
        );
    }
    image
}

/// The choices sheet in `theme` with its checks: the progress bar filled in
/// the accent to 60 %, only the checked radio button has a dot, equal
/// panes, every control's text centred, clear of its edges and 4.5:1.
fn checked_choices(painter: &Painter, theme: UiTheme) -> Image {
    let mut screen = painter.screen(CHOICES);
    let s = choices(&mut screen, theme);
    frame(&mut screen, CHOICES, OFF, false, 0.0);
    let image = draw(painter, &mut screen, CHOICES);
    let ui = &screen.ctx.ui;
    let rect = |id| ui.rect(id).unwrap();
    let bar = rect(s.progress);
    let filled = at(&image, vec2(bar.min.x + bar.size().x * 0.3, bar.center().y));
    let empty = at(&image, vec2(bar.min.x + bar.size().x * 0.8, bar.center().y));
    assert!(
        near(filled, theme.accent.to_rgba8()) && far(filled, empty),
        "{filled:?} {empty:?}"
    );
    let (left, right) = (rect(s.panes.0), rect(s.panes.1));
    assert!((left.size().x - right.size().x).abs() <= 1.0, "{left:?} {right:?}");

    let gap = theme.spacing * 0.75;
    // The circle (or box) is as tall as the row, at most 20; the text is
    // 6 past it.
    let after_mark = |r: Rect| {
        let mark = r.size().y.min(20.0);
        Rect::new(
            r.min.x + gap + mark + 3.0,
            r.min.y + 3.0,
            r.size().x - gap - mark - 6.0,
            r.size().y - 6.0,
        )
    };
    let mut dots = [0; 3];
    for (i, &id) in s.radios.iter().enumerate() {
        let r = rect(id);
        check_text(
            &image,
            "radio",
            r,
            after_mark(r),
            vec2(r.max.x - 3.0, r.center().y),
            gap,
        );
        // The dot: the text's color in the middle of the circle.
        let side = r.size().y.min(20.0);
        let circle = Rect::new(r.min.x + gap, r.center().y - side * 0.5, side, side);
        dots[i] = ink(
            &image,
            circle.expand(-6.0),
            at(&image, vec2(r.max.x - 3.0, r.center().y)),
        )
        .iter()
        .filter(|&&(_, _, p)| near(p, theme.text.to_rgba8()))
        .count();
    }
    assert!(
        dots[1] > 0 && dots[0] == 0 && dots[2] == 0,
        "only the chosen radio has a dot: {dots:?}"
    );
    let m = rect(s.music);
    check_text(
        &image,
        "checkbox",
        m,
        after_mark(m),
        vec2(m.max.x - 3.0, m.center().y),
        gap,
    );
    image
}

#[test]
fn every_widget_draws_its_state_in_the_dark_and_light_themes() {
    let Some(painter) = Painter::new() else { return };
    for (name, theme, other_theme, controls_reference, choices_reference) in [
        (
            "dark",
            UiTheme::dark(),
            UiTheme::light(),
            "ui_widgets_controls_dark",
            "ui_widgets_choices_dark",
        ),
        (
            "light",
            UiTheme::light(),
            UiTheme::dark(),
            "ui_widgets_controls_light",
            "ui_widgets_choices_light",
        ),
    ] {
        let controls = checked_controls(&painter, theme);
        let other = checked_controls(&painter, other_theme);
        // The light theme's panels are light, the dark theme's dark.
        let corner = controls.pixel(4, 4);
        assert_eq!(corner[0] > 128, name == "light", "{corner:?}");
        let reference = controls_reference;
        check_reference(
            reference,
            &controls,
            REFERENCE,
            &format!(
                "UiTheme::{name}() (4 unit corner radius), 256x168, a column with 10 unit padding: a dropdown \
                 showing \"High\" with its arrow and a number field \"5\" between - and +; a key-binding field \
                 listening (\"Press a key\", wide border) and an \"Apply\" button; a tab strip with \"Audio\" \
                 chosen and underlined in the accent above its page's label. Every control's text centred on its \
                 height, the button's, number's and key binding's across too, three quarters of the theme's spacing from the \
                 edges at least; {PIPELINE}"
            ),
            &painter.gpu.info,
        );
        reference_rejects(reference, &other, REFERENCE, "the other theme");

        let choices = checked_choices(&painter, theme);
        let other = checked_choices(&painter, other_theme);
        let reference = choices_reference;
        check_reference(
            reference,
            &choices,
            REFERENCE,
            &format!(
                "UiTheme::{name}(), 256x160, a column with 10 unit padding: three radio buttons Off, Low, High \
                 with Low checked; a checked \"Music\" checkbox and a spinner; a progress bar 60 % filled in the \
                 accent; equal split panes \"Left\" and \"Right\" with the divider's grip between. Circles, box and \
                 text centred on their rows; {PIPELINE}"
            ),
            &painter.gpu.info,
        );
        reference_rejects(reference, &other, REFERENCE, "the other theme");
    }
}

/// The controls sheet, nothing open, the key-binding field not listening.
fn closed_controls(painter: &Painter) -> Image {
    let mut screen = painter.screen(CONTROLS);
    controls(&mut screen, UiTheme::dark(), false);
    frame(&mut screen, CONTROLS, OFF, false, 0.0);
    draw(painter, &mut screen, CONTROLS)
}

/// The choices sheet, nothing open or hovered.
fn closed_choices(painter: &Painter) -> Image {
    let mut screen = painter.screen(CHOICES);
    choices(&mut screen, UiTheme::dark());
    frame(&mut screen, CHOICES, OFF, false, 0.0);
    draw(painter, &mut screen, CHOICES)
}

#[test]
fn pop_ups_and_tooltips_draw_over_the_sheet() {
    let Some(painter) = Painter::new() else { return };

    // The dropdown, opened as assistive technology would: its options
    // below it, the chosen one ("High", focused) tinted, "Low" not.
    let mut screen = painter.screen(CONTROLS);
    let s = controls(&mut screen, UiTheme::dark(), false);
    frame(&mut screen, CONTROLS, OFF, false, 0.0);
    screen.ctx.ui.push_access_action(UiAccessAction::Activate(s.dropdown));
    frame(&mut screen, CONTROLS, OFF, false, 0.0);
    assert_eq!(screen.ctx.ui.popup_owner(), Some(s.dropdown));
    let image = draw(&painter, &mut screen, CONTROLS);
    let ui = &screen.ctx.ui;
    let menu = ui.top_screen().unwrap();
    let rows = ui.accessibility_tree().get(menu).unwrap().children.clone();
    let (low, high) = (ui.rect(rows[0]).unwrap(), ui.rect(rows[2]).unwrap());
    let end = |r: Rect| at(&image, vec2(r.max.x - 4.0, r.center().y));
    assert!(far(end(low), end(high)), "{:?} {:?}", end(low), end(high));
    let gap = UiTheme::dark().spacing * 0.75;
    for (what, r) in [("option", low), ("chosen option", high)] {
        // Clear of the focused option's border.
        check_text(&image, what, r, r.expand(-3.0), vec2(r.max.x - 4.0, r.center().y), gap);
    }
    check_reference(
        "ui_widgets_dropdown",
        &image,
        REFERENCE,
        &format!(
            "the ui_widgets_controls_dark sheet, its key-binding field not listening (\"Space\"), with the dropdown \
             open below itself: Low, Medium, High on a control-colored panel with a border, High chosen, tinted in \
             the accent and focused; {PIPELINE}"
        ),
        &painter.gpu.info,
    );
    reference_rejects(
        "ui_widgets_dropdown",
        &closed_controls(&painter),
        REFERENCE,
        "the sheet with nothing open",
    );

    let closed = closed_choices(&painter);
    // The checkbox's context menu, opened by the menu input with it focused.
    let mut screen = painter.screen(CHOICES);
    let s = choices(&mut screen, UiTheme::dark());
    frame(&mut screen, CHOICES, OFF, false, 0.0);
    screen.ctx.ui.set_focus(s.music);
    screen.ctx.ui.frame(
        CHOICES,
        &UiInput {
            pointer: OFF,
            menu: true,
            ..Default::default()
        },
    );
    assert_eq!(screen.ctx.ui.popup_owner(), Some(s.music));
    let image = draw(&painter, &mut screen, CHOICES);
    let ui = &screen.ctx.ui;
    let menu = ui.top_screen().unwrap();
    for &row in &ui.accessibility_tree().get(menu).unwrap().children {
        let r = ui.rect(row).unwrap();
        check_text(
            &image,
            "menu item",
            r,
            r.expand(-3.0),
            vec2(r.max.x - 4.0, r.center().y),
            gap,
        );
    }
    check_reference(
        "ui_widgets_menu",
        &image,
        REFERENCE,
        &format!(
            "the ui_widgets_choices_dark sheet with the \"Music\" checkbox focused and its context menu (Mute, Solo) \
             open below it, \"Mute\" focused; {PIPELINE}"
        ),
        &painter.gpu.info,
    );
    reference_rejects("ui_widgets_menu", &closed, REFERENCE, "the sheet with nothing open");

    // The pointer rests on the checkbox until its tooltip shows: a box with
    // an accent border and the text, below the pointer.
    let mut screen = painter.screen(CHOICES);
    let s = choices(&mut screen, UiTheme::dark());
    frame(&mut screen, CHOICES, OFF, false, 0.0);
    let over = screen.ctx.ui.rect(s.music).unwrap().center();
    for _ in 0..3 {
        frame(&mut screen, CHOICES, over, false, 0.3);
    }
    let (whose, tip) = screen.ctx.ui.shown_tooltip().unwrap();
    assert_eq!(whose, s.music);
    assert!(tip.min.y > over.y && tip.max.x <= CHOICES.x, "{tip:?}");
    let image = draw(&painter, &mut screen, CHOICES);
    let border = at(&image, tip.min + vec2(tip.size().x * 0.5, 0.5));
    let inside = at(&image, vec2(tip.min.x + 4.0, tip.center().y));
    assert!(far(border, inside), "{border:?} {inside:?}");
    check_text(
        &image,
        "tooltip",
        tip,
        tip.expand(-3.0),
        vec2(tip.min.x + 4.0, tip.center().y),
        gap,
    );
    check_reference(
        "ui_widgets_tooltip",
        &image,
        REFERENCE,
        &format!(
            "the ui_widgets_choices_dark sheet, the pointer on the \"Music\" checkbox (hovered) and its tooltip \
             \"Plays the soundtrack\" in a box with an accent border below the pointer, kept on the screen; \
             {PIPELINE}"
        ),
        &painter.gpu.info,
    );
    reference_rejects("ui_widgets_tooltip", &closed, REFERENCE, "the sheet with no tooltip");
}

/// A list, a grid and two containers with an item in the first; `drag`
/// carries it most of the way to the second, pointer held.
fn inventory(painter: &Painter, drag: bool) -> (Image, Screen, UiId, UiId) {
    let mut screen = painter.screen(INVENTORY);
    let root = root(&mut screen, UiTheme::dark());
    let ui = &mut screen.ctx.ui;
    let top = ui.panel(root).layout(UiLayout::row(96.0).gap(8.0)).id();
    let list = ui.list(top, 24.0).layout(UiLayout::fixed(114.0, 96.0)).id();
    ui.set_items(list, (0..10_000).map(|i| format!("Save {i}")));
    ui.set_selected(list, Some(2));
    let grid = ui.grid_view(top, 4, 32.0).layout(UiLayout::fixed(114.0, 96.0)).id();
    ui.set_items(grid, (0..40).map(|i| format!("{i}")));
    ui.set_selected(grid, Some(5));
    let bottom = ui.panel(root).layout(UiLayout::row(72.0).gap(8.0)).id();
    let bag = ui.panel(bottom).layout(UiLayout::fixed(114.0, 72.0)).drop_target().id();
    let chest = ui.panel(bottom).layout(UiLayout::fixed(114.0, 72.0)).drop_target().id();
    let gem = ui
        .button(bag, "Gem")
        .layout(UiLayout::fixed(64.0, 40.0))
        .draggable()
        .id();
    frame(&mut screen, INVENTORY, OFF, false, 0.0);
    if drag {
        let ui = &mut screen.ctx.ui;
        let from = ui.rect(gem).unwrap().center();
        let to = ui.rect(chest).unwrap().center();
        ui.frame(
            INVENTORY,
            &UiInput {
                pointer: from,
                pressed: true,
                held: true,
                ..Default::default()
            },
        );
        frame(&mut screen, INVENTORY, to, true, 0.0);
        assert_eq!(screen.ctx.ui.dragging(), Some(gem));
    }
    (draw(painter, &mut screen, INVENTORY), screen, chest, gem)
}

#[test]
fn lists_grids_and_a_dragged_item_draw_where_they_belong() {
    let Some(painter) = Painter::new() else { return };
    let (still, screen, _, gem) = inventory(&painter, false);
    // The item's text is centred in it, clear of its border.
    let r = screen.ctx.ui.rect(gem).unwrap();
    let gap = UiTheme::dark().spacing * 0.75;
    check_text(
        &still,
        "item",
        r,
        r.expand(-2.0),
        vec2(r.min.x + 3.0, r.center().y),
        gap,
    );
    let (image, screen, chest, _) = inventory(&painter, true);
    // The chest is outlined in the accent where the item would land.
    let r = screen.ctx.ui.rect(chest).unwrap();
    let edge = at(&image, vec2(r.center().x, r.min.y + 0.5));
    assert!(far(edge, at(&still, vec2(r.center().x, r.min.y + 0.5))), "{edge:?}");
    check_reference(
        "ui_widgets_drag",
        &image,
        REFERENCE,
        &format!(
            "UiTheme::dark(), 256x200: a list of 10,000 saves showing its first rows with \"Save 2\" chosen and \
             tinted, a 4-column grid of numbered cells with 5 chosen; below, two containers, the \"Gem\" button \
             (text centred) in the left one, outlined while dragged, and a see-through copy of it over the right \
             one, which is outlined in the accent; {PIPELINE}"
        ),
        &painter.gpu.info,
    );
    reference_rejects("ui_widgets_drag", &still, REFERENCE, "the same screen before the drag");
}

#[test]
fn developer_console_draws_and_clips_scrollback_above_the_game_ui() {
    use crate::input::Key;

    let Some(painter) = Painter::new() else { return };
    let size = vec2(480.0, 280.0);
    let mut screen = painter.screen(size);
    let root = screen
        .ctx
        .ui
        .root()
        .layout(UiLayout::fill())
        .background(crate::color::Color::rgb(0.7, 0.1, 0.1))
        .pointer_events(false)
        .id();
    screen.ctx.ui.push_modeless(root);

    screen.ctx.update_retained_ui();
    let before = draw(&painter, &mut screen, size);
    let ctx = &mut screen.ctx;
    ctx.console_registry.enabled = Some(true);
    ctx.console_registry
        .command(
            "dump".into(),
            "Print a report".into(),
            &[],
            Box::new(|_, _| {
                Ok((0..20)
                    .map(|i| format!("Report line {i}"))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }),
        )
        .unwrap();
    ctx.input.set_key(Key::Backquote, true);
    ctx.update_retained_ui();
    ctx.input.end_frame();
    ctx.input.insert_text("dump");
    ctx.input.set_key(Key::Enter, true);
    ctx.update_retained_ui();
    ctx.input.end_frame();
    ctx.update_retained_ui();
    let scroll = ctx.ui.find("ponko.console.scroll").unwrap();
    let scroll_rect = ctx.ui.rect(scroll).unwrap();
    assert!(ctx.ui.scroll(scroll).unwrap().y > scroll_rect.size().y);
    let opened = draw(&painter, &mut screen, size);
    let panel = opened.pixel(4, 4);
    assert_ne!(panel, before.pixel(4, 4));
    assert!(
        panel[2] > panel[0],
        "the console shows its blue panel above the game UI"
    );
    let below = opened.pixel(20, 255);
    assert!(below[0] > below[2], "scrollback stays above the bottom of the console");
    assert!(
        (scroll_rect.min.y as u32..scroll_rect.max.y as u32).any(|y| (12..130).any(|x| opened.pixel(x, y)[0] > 190)),
        "scrolled output draws readable text inside the clip",
    );

    let ctx = &mut screen.ctx;
    ctx.input.set_key(Key::PageUp, true);
    let at_bottom = ctx.ui.scroll(scroll).unwrap().y;
    ctx.update_retained_ui();
    assert!(ctx.ui.scroll(scroll).unwrap().y < at_bottom);
    let earlier = draw(&painter, &mut screen, size);
    assert_ne!(earlier.rgba, opened.rgba);
}
