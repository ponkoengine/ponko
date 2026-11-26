//! A retained shop: item grid, scrolling, input and modal confirmation.
use ponko::prelude::*;
#[allow(unused_imports)]
use ponko::ui::{UiEvent, UiLayout};

pub fn build(ctx: &mut Ctx) {
    // ANCHOR: shop_tree
    let ui = &mut ctx.ui;
    let root = ui.root().layout(UiLayout::fill().padding(24.0).gap(12.0)).id();
    let title = ui.label(root, "General store — choose an item");
    title.layout(UiLayout::fixed(400.0, 32.0)).name("title");
    let note = ui.text_input(root).text("Your order note");
    note.layout(UiLayout::fixed(360.0, 36.0)).name("note");
    let scroll = ui.scroll_area(root).layout(UiLayout::fill().height(360.0)).id();
    // 95% wide, so the scrollbar doesn't cover the last column.
    let cells = UiLayout::grid(3, 76.0).gap(8.0).width_fraction(0.95);
    let grid = ui.panel(scroll).layout(cells).id();
    for i in 1..=36 {
        let item = ui.button(grid, format!("Item {i}\n{} coins", i * 3));
        let item = item.layout(UiLayout::auto().padding(8.0)).wrap().border_width(1.0).id();
        ui.set_name(item, format!("item.{i}"));
    }
    ui.toggle(root, "Gift wrap")
        .layout(UiLayout::fixed(220.0, 28.0))
        .name("gift");
    let buy = ui.button(root, "Buy selected item");
    buy.layout(UiLayout::fixed(220.0, 40.0)).enabled(false).name("buy");
    ui.push_modeless(root);
    // ANCHOR_END: shop_tree

    // ANCHOR: shop_dialog
    // A detached screen can be reused after it is popped: 180 pixels from the
    // top here (without `top`, in the middle of the window).
    let panel = UiLayout::centered(360.0, 180.0).top(180.0).padding(20.0).gap(12.0);
    let dialog = ui.root().layout(panel).name("dialog").id();
    ui.label(dialog, "")
        .layout(UiLayout::fixed(320.0, 40.0))
        .name("message");
    let confirm = ui.button(dialog, "Confirm purchase");
    confirm.layout(UiLayout::fixed(240.0, 36.0)).name("confirm");
    ui.button(dialog, "Back")
        .layout(UiLayout::fixed(240.0, 30.0))
        .name("cancel");
    // ANCHOR_END: shop_dialog
}

// ANCHOR: shop_events
pub fn shop(ctx: &mut Ctx) {
    // UI input and layout ran before this system. Names are looked up only
    // when something was clicked, not every frame.
    let ui = &mut ctx.ui;
    if let Some(clicked) = ui.events().filter_map(UiEvent::clicked).last() {
        let [title, buy, message, dialog] = ["title", "buy", "message", "dialog"].map(|n| ui.try_find(n).unwrap());
        let clicked = ui.name(clicked).unwrap_or_default().to_owned();
        if let Some(item) = clicked.strip_prefix("item.") {
            ui.set_enabled(buy, true);
            ui.set_text(buy, format!("Buy item {item}"));
            ui.set_text(message, format!("Buy item {item}?"));
        } else if clicked == "buy" {
            ui.push_modal(dialog);
        } else if clicked == "confirm" || clicked == "cancel" {
            ui.pop_screen();
            if clicked == "confirm" {
                ui.set_text(title, "Purchase complete!");
            }
        }
    }
    // Tab/arrows or d-pad navigate; Enter/South confirms; Escape/East
    // pops the top screen. A modal screen also owns the outside pointer.
    if !ctx.ui.wants_pointer() { /* world pointer actions */ }
    if !ctx.ui.wants_keyboard() { /* world keyboard/gamepad actions */ }
}
// ANCHOR_END: shop_events

fn main() {
    Game::new("Shop", 900, 700).system(shop).setup(build).run();
}
