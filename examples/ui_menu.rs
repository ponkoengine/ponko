//! A small settings menu that a screen reader can read and operate.
//!
//!   `cargo run --example ui_menu --features accessibility`
//!
//! Every control has a name: the buttons and the toggle from their text, the
//! slider and the text input from `accessible_name`. The status line is a
//! live region, so a change to it is announced while focus is elsewhere.
//! "Quit" opens a modal dialog, which is all a screen reader sees until it
//! is closed.
//!
//! With the `accessibility` feature the engine gives `ctx.ui` to the
//! system's accessibility API (macOS: VoiceOver, Cmd-F5). Without it the
//! menu works the same with the keyboard, a gamepad and the mouse: Tab and
//! the arrow keys move focus, Enter presses, Escape closes the dialog.

use ponko::prelude::*;
use ponko::ui::{Ui, UiEvent, UiId, UiLayout, UiLive};

/// The menu's nodes.
// ANCHOR: menu
pub struct Menu {
    pub root: UiId,
    pub status: UiId,
    pub play: UiId,
    pub volume: UiId,
    pub name: UiId,
    pub music: UiId,
    pub quit: UiId,
    pub dialog: UiId,
    pub yes: UiId,
    pub no: UiId,
}

/// Builds the menu and its quit dialog; the menu is the open screen.
pub fn build(ui: &mut Ui) -> Menu {
    let root = ui.root().layout(UiLayout::fill().padding(24.0).gap(10.0)).id();
    ui.title(root, "Settings").layout(UiLayout::row(40.0));
    let status = ui
        .label(root, "Ready")
        .layout(UiLayout::row(24.0))
        .live_region(UiLive::Polite)
        .id();
    let play = ui
        .button(root, "Play")
        .layout(UiLayout::fixed(240.0, 40.0))
        .accessible_description("Starts a new run")
        .id();
    // A slider and a text input have no text of their own to be named by.
    let volume = ui
        .slider(root)
        .layout(UiLayout::fixed(240.0, 24.0))
        .value(0.5)
        .accessible_name("Volume")
        .id();
    let name = ui
        .text_input(root)
        .layout(UiLayout::fixed(240.0, 36.0))
        .text("Ann")
        .accessible_name("Player name")
        .id();
    let music = ui
        .toggle(root, "Music")
        .layout(UiLayout::fixed(240.0, 28.0))
        .value(1.0)
        .id();
    let quit = ui.button(root, "Quit").layout(UiLayout::fixed(240.0, 40.0)).id();
    ui.push_modeless(root);

    let dialog = ui
        .root()
        .layout(UiLayout::centered(320.0, 170.0).padding(20.0).gap(10.0))
        .accessible_name("Quit the game?")
        .id();
    ui.label(dialog, "Quit the game?").layout(UiLayout::row(30.0));
    let yes = ui.button(dialog, "Yes").layout(UiLayout::row(36.0)).id();
    let no = ui.button(dialog, "No").layout(UiLayout::row(36.0)).id();
    Menu {
        root,
        status,
        play,
        volume,
        name,
        music,
        quit,
        dialog,
        yes,
        no,
    }
}
// ANCHOR_END: menu

/// What the menu does with one UI event. True when the game should quit.
pub fn react(ui: &mut Ui, menu: &Menu, event: UiEvent) -> bool {
    match event {
        UiEvent::Clicked(id) if id == menu.play => {
            let name = ui.text(menu.name).unwrap_or_default().to_owned();
            ui.set_text(menu.status, format!("Started a run for {name}"));
        }
        UiEvent::Clicked(id) if id == menu.quit => ui.push_modal(menu.dialog),
        UiEvent::Clicked(id) if id == menu.no => {
            ui.pop_screen();
        }
        UiEvent::Clicked(id) if id == menu.yes => return true,
        UiEvent::Changed(id) if id == menu.volume => {
            let percent = (ui.value(id).unwrap_or(0.0) * 100.0).round();
            ui.set_text(menu.status, format!("Volume {percent}%"));
        }
        UiEvent::Changed(id) if id == menu.music => {
            let on = ui.value(id).unwrap_or(0.0) >= 0.5;
            ui.set_text(menu.status, if on { "Music on" } else { "Music off" });
        }
        _ => {}
    }
    false
}

fn setup(ctx: &mut Ctx) {
    let menu = build(&mut ctx.ui);
    ctx.world.insert_resource(menu);
}

fn events(ctx: &mut Ctx) {
    let menu = ctx.world.remove_resource::<Menu>().expect("setup made the menu");
    while let Some(event) = ctx.ui.next_event() {
        if react(&mut ctx.ui, &menu, event) {
            ctx.quit();
        }
    }
    ctx.world.insert_resource(menu);
}

fn main() {
    Game::new("Settings", 520, 460).system(events).setup(setup).run();
}
