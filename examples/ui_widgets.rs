//! The retained UI's widget set: a settings page and an inventory.
//!
//!   `cargo run --example ui_widgets`
//!
//! The settings page has tabs, a dropdown, radio buttons, a number field, a
//! progress bar, a spinner and a key binding; the inventory a virtualized
//! grid of 10 000 cells, split panes, drag and drop and a context menu.
//! "Reset" opens a modal dialog that keeps the focus inside it. T switches
//! between the light theme and a custom dark one.
//!
//! Everything works with the mouse, the keyboard (Tab and arrows move,
//! Enter activates, Escape closes) and a gamepad, and every widget has a
//! screen-reader role.

use ponko::prelude::*;
use ponko::ui::{Ui, UiAxis, UiEvent, UiId, UiLayout, UiTheme};

/// The gallery's nodes.
pub struct Gallery {
    pub quality: UiId,
    pub status: UiId,
    pub bag: UiId,
    pub chest: UiId,
    pub reset: UiId,
    pub dialog: UiId,
    pub confirm: UiId,
}

/// Builds the gallery; the settings page is the open screen.
pub fn build(ui: &mut Ui) -> Gallery {
    // ANCHOR: settings
    let root = ui.root().layout(UiLayout::fill().padding(24.0).gap(12.0)).id();
    let tabs = ui.tabs(root).layout(UiLayout::fill()).id();
    let video = ui.tab(tabs, "Video").id();
    let controls = ui.tab(tabs, "Controls").id();
    let inventory = ui.tab(tabs, "Inventory").id();

    let quality = ui
        .dropdown(video, ["Low", "Medium", "High"])
        .layout(UiLayout::fixed(220.0, 32.0))
        .selected(1)
        .tooltip("How detailed the world looks")
        .id();
    let vsync = ui.radio_group(video).id();
    ui.radio(vsync, "Off").layout(UiLayout::fixed(160.0, 24.0));
    ui.radio(vsync, "On").layout(UiLayout::fixed(160.0, 24.0));
    ui.number_field(video, 90.0)
        .layout(UiLayout::fixed(140.0, 28.0))
        .range(60.0, 120.0)
        .step(5.0)
        .accessible_name("Field of view");
    ui.progress_bar(video).layout(UiLayout::fixed(220.0, 8.0)).value(0.6);
    ui.spinner(video);
    ui.key_binding(controls, Some(ButtonBinding::Key(Key::Space)))
        .layout(UiLayout::fixed(200.0, 28.0))
        .accessible_name("Jump");
    // ANCHOR_END: settings

    // ANCHOR: inventory
    // A grid keeps nodes only for the cells on screen: 10 000 cost what 40 do.
    let split = ui.split(inventory, UiAxis::Horizontal).layout(UiLayout::fill()).id();
    let (bag, chest) = ui.panes(split).expect("split panes have two panes");
    let cells = ui.grid_view(bag, 4, 48.0).layout(UiLayout::fill()).id();
    ui.set_list_len(cells, 10_000);
    ui.button(chest, "Gem")
        .layout(UiLayout::fixed(64.0, 48.0))
        .draggable()
        .context_menu(["Use", "Drop"]);
    ui.set_drop_target(bag, true);
    ui.set_drop_target(chest, true);
    // ANCHOR_END: inventory

    let status = ui.label(root, "Ready").layout(UiLayout::row(24.0)).id();
    let reset = ui.button(root, "Reset").layout(UiLayout::fixed(160.0, 36.0)).id();
    ui.push_modeless(root);

    // ANCHOR: dialog
    // A modal screen: the focus stays inside it, the pointer can't reach
    // what is below, and a screen reader sees only it.
    let dialog = ui
        .root()
        .layout(UiLayout::centered(320.0, 150.0).padding(20.0).gap(10.0))
        .accessible_name("Reset every setting?")
        .id();
    ui.label(dialog, "Reset every setting?").layout(UiLayout::row(28.0));
    let confirm = ui.button(dialog, "Reset").layout(UiLayout::row(36.0)).id();
    ui.button(dialog, "Cancel").layout(UiLayout::row(36.0)).closes_screen();
    // ANCHOR_END: dialog
    Gallery {
        quality,
        status,
        bag,
        chest,
        reset,
        dialog,
        confirm,
    }
}

/// What the gallery does with one UI event.
// ANCHOR: events
pub fn react(ui: &mut Ui, g: &Gallery, event: UiEvent) {
    match event {
        UiEvent::Changed(id) if id == g.quality => {
            let choice = ui.selected(id).and_then(|i| ui.items(id)?.get(i).cloned());
            ui.set_text(g.status, format!("Quality: {}", choice.unwrap_or_default()));
        }
        UiEvent::Chosen(item, entry) => {
            let name = ui.text(item).unwrap_or_default().to_owned();
            ui.set_text(g.status, format!("{name}: entry {entry}"));
        }
        // The game decides whether a drop is allowed, and moves the node.
        UiEvent::Dropped(item, onto) if onto == g.bag || onto == g.chest => {
            ui.set_parent(item, onto);
        }
        UiEvent::Clicked(id) if id == g.reset => ui.push_modal(g.dialog),
        UiEvent::Clicked(id) if id == g.confirm => {
            ui.pop_screen();
            ui.set_selected(g.quality, Some(1));
            ui.set_text(g.status, "Reset");
        }
        _ => {}
    }
}
// ANCHOR_END: events

/// A custom theme: the dark one with square corners, more room and a warm
/// accent.
// ANCHOR: theme
pub fn custom_theme() -> UiTheme {
    UiTheme {
        accent: Color::hex(0xe0803a),
        spacing: 12.0,
        radius: 0.0,
        ..UiTheme::dark()
    }
}
// ANCHOR_END: theme

fn setup(ctx: &mut Ctx) {
    ctx.ui.set_theme(UiTheme::light());
    let gallery = build(&mut ctx.ui);
    ctx.world.insert_resource(gallery);
}

fn events(ctx: &mut Ctx) {
    let gallery = ctx.world.remove_resource::<Gallery>().expect("setup built the gallery");
    while let Some(event) = ctx.ui.next_event() {
        react(&mut ctx.ui, &gallery, event);
    }
    // T switches themes, unless a key-binding field is recording it.
    let recording = ctx.ui.focus().is_some_and(|f| ctx.ui.is_capturing(f));
    if ctx.input.key_pressed(Key::T) && !recording {
        let light = ctx.ui.theme().panel.r > 0.5;
        ctx.ui.set_theme(if light { custom_theme() } else { UiTheme::light() });
    }
    ctx.world.insert_resource(gallery);
}

fn main() {
    Game::new("Widgets", 720, 560).system(events).setup(setup).run();
}
