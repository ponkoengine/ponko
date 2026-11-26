//! The widget set, driven headless: every widget by pointer, keyboard and
//! gamepad (the same `UiInput` fields a gamepad's d-pad, South and East
//! buttons fill), what assistive technology sees of it, and the list's
//! flat cost.
use ponko::prelude::*;
use ponko::ui::{
    Ui, UiAccessAction, UiAccessValue, UiAxis, UiEvent, UiId, UiInput, UiLayout, UiRole, UiTheme, UiWidget,
};

const VIEW: Vec2 = Vec2::new(640.0, 480.0);

fn screen() -> (Ui, UiId) {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(6.0)).id();
    ui.push_modeless(root);
    (ui, root)
}

fn idle(ui: &mut Ui) -> Vec<UiEvent> {
    step(ui, UiInput::default())
}

fn step(ui: &mut Ui, input: UiInput) -> Vec<UiEvent> {
    ui.frame(VIEW, &input);
    ui.events().collect()
}

fn click(ui: &mut Ui, at: Vec2) -> Vec<UiEvent> {
    let mut events = step(
        ui,
        UiInput {
            pointer: at,
            pressed: true,
            held: true,
            ..Default::default()
        },
    );
    events.extend(step(
        ui,
        UiInput {
            pointer: at,
            released: true,
            ..Default::default()
        },
    ));
    events
}

fn click_on(ui: &mut Ui, id: UiId) -> Vec<UiEvent> {
    let at = center(ui, id);
    click(ui, at)
}

fn center(ui: &Ui, id: UiId) -> Vec2 {
    ui.rect(id).expect("a laid out node").center()
}

fn confirm() -> UiInput {
    UiInput {
        confirm: true,
        ..Default::default()
    }
}

fn arrow(x: f32, y: f32) -> UiInput {
    UiInput {
        direction: vec2(x, y),
        ..Default::default()
    }
}

fn cancel() -> UiInput {
    UiInput {
        cancel: true,
        ..Default::default()
    }
}

// ---- dropdown ---------------------------------------------------------------

fn quality() -> (Ui, UiId, UiId) {
    let (mut ui, root) = screen();
    let dropdown = ui
        .dropdown(root, ["Low", "Medium", "High"])
        .layout(UiLayout::fixed(160.0, 32.0))
        .accessible_name("Quality")
        .id();
    let below = ui.button(root, "Apply").layout(UiLayout::fixed(160.0, 32.0)).id();
    idle(&mut ui);
    (ui, dropdown, below)
}

#[test]
fn a_dropdown_chooses_an_option_with_the_pointer() {
    let (mut ui, dropdown, _) = quality();
    assert_eq!((ui.selected(dropdown), ui.text(dropdown)), (Some(0), Some("Low")));
    click_on(&mut ui, dropdown);
    assert_eq!(ui.popup_owner(), Some(dropdown));
    let menu = ui.top_screen().unwrap();
    assert_ne!(
        menu,
        ui.parent(dropdown).unwrap(),
        "the options are a screen of their own"
    );
    // The options open below it, as wide as it.
    let (field, options) = (ui.rect(dropdown).unwrap(), ui.rect(menu).unwrap());
    assert!(options.min.y >= field.max.y - 0.5 && options.size().x >= field.size().x);
    let high = ui.accessibility_tree().get(menu).unwrap().children[2];
    let events = click_on(&mut ui, high);
    assert!(events.contains(&UiEvent::Changed(dropdown)), "{events:?}");
    assert_eq!((ui.selected(dropdown), ui.text(dropdown)), (Some(2), Some("High")));
    assert_eq!(ui.popup_owner(), None);
    assert_eq!(ui.focus(), Some(dropdown), "the focus goes back to the dropdown");
}

#[test]
fn a_dropdown_chooses_with_the_keyboard_and_a_gamepad() {
    let (mut ui, dropdown, _) = quality();
    ui.set_focus(dropdown);
    // Confirm is Enter and a gamepad's South button; the arrows are the d-pad.
    step(&mut ui, confirm());
    let focused = ui.focus().unwrap();
    assert_eq!(ui.text(focused), Some("Low"), "the chosen option has the focus");
    step(&mut ui, arrow(0.0, 1.0));
    step(&mut ui, arrow(0.0, 1.0));
    let events = step(&mut ui, confirm());
    assert_eq!(events, [UiEvent::Changed(dropdown)]);
    assert_eq!(ui.selected(dropdown), Some(2));
}

#[test]
fn escape_closes_a_dropdown_without_closing_its_screen() {
    let (mut ui, dropdown, _) = quality();
    let root = ui.top_screen().unwrap();
    ui.set_focus(dropdown);
    step(&mut ui, confirm());
    step(&mut ui, arrow(0.0, 1.0));
    let events = step(&mut ui, cancel());
    assert!(events.is_empty(), "no screen popped, nothing chosen: {events:?}");
    assert_eq!((ui.popup_owner(), ui.top_screen()), (None, Some(root)));
    assert_eq!(ui.selected(dropdown), Some(0));
}

#[test]
fn a_press_outside_an_open_dropdown_closes_it_and_presses_nothing_else() {
    let (mut ui, dropdown, below) = quality();
    click_on(&mut ui, dropdown);
    // The options cover the button below; press far away instead.
    let events = click(&mut ui, vec2(600.0, 460.0));
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(ui.popup_owner(), None);
    let events = click_on(&mut ui, below);
    assert_eq!(events, [UiEvent::Clicked(below)], "closed for good");
}

#[test]
fn a_dropdown_is_a_combo_box_with_selected_options() {
    let (mut ui, dropdown, _) = quality();
    let tree = ui.accessibility_tree();
    let node = tree.get(dropdown).unwrap();
    assert_eq!((node.role, node.name.as_str()), (UiRole::ComboBox, "Quality"));
    assert_eq!(node.value, UiAccessValue::Text("Low".into()));
    assert_eq!(node.expanded, Some(false));
    ui.push_access_action(UiAccessAction::Activate(dropdown));
    idle(&mut ui);
    let tree = ui.accessibility_tree();
    assert_eq!(tree.get(dropdown).unwrap().expanded, Some(true));
    let menu = ui.top_screen().unwrap();
    assert_eq!(tree.get(menu).unwrap().role, UiRole::List);
    let options: Vec<_> = tree
        .get(menu)
        .unwrap()
        .children
        .iter()
        .map(|&c| tree.get(c).unwrap())
        .collect();
    assert_eq!(options.len(), 3);
    assert!(options.iter().all(|o| o.role == UiRole::ListItem));
    assert_eq!(options[0].position, Some((1, 3)));
    assert!(options[0].selected && !options[1].selected);
    // Assistive technology chooses by index too.
    ui.close_popup();
    ui.push_access_action(UiAccessAction::SetValue(dropdown, 1.0));
    assert_eq!(idle(&mut ui), [UiEvent::Changed(dropdown)]);
    assert_eq!(ui.text(dropdown), Some("Medium"));
}

// ---- tabs ---------------------------------------------------------------------

fn settings() -> (Ui, UiId, [UiId; 3]) {
    let (mut ui, root) = screen();
    let tabs = ui.tabs(root).layout(UiLayout::fill()).id();
    let pages = ["General", "Audio", "Video"].map(|title| ui.tab(tabs, title).id());
    ui.label(pages[0], "Language");
    ui.slider(pages[1]).layout(UiLayout::row(20.0)).accessible_name("Music");
    ui.toggle(pages[2], "V-sync").layout(UiLayout::fixed(120.0, 28.0));
    idle(&mut ui);
    (ui, tabs, pages)
}

#[test]
fn only_the_chosen_tabs_page_is_shown() {
    let (mut ui, tabs, pages) = settings();
    assert_eq!(ui.selected(tabs), Some(0));
    assert!(ui.is_shown(pages[0]) && !ui.is_shown(pages[1]) && !ui.is_shown(pages[2]));
    let video = ui.tab_button(tabs, 2).unwrap();
    let events = click_on(&mut ui, video);
    assert_eq!(events, [UiEvent::Changed(tabs)]);
    assert!(!ui.is_shown(pages[0]) && ui.is_shown(pages[2]));
    // The page fills the strip under the bar.
    let (bar, page) = (ui.rect(video).unwrap(), ui.rect(pages[2]).unwrap());
    assert!(page.min.y >= bar.max.y && page.size().y > 300.0, "{page:?}");
    // Choosing it again changes nothing.
    assert!(click_on(&mut ui, video).is_empty());
}

#[test]
fn tabs_switch_with_the_keyboard_and_a_gamepad() {
    let (mut ui, tabs, pages) = settings();
    ui.set_focus(ui.tab_button(tabs, 0).unwrap());
    step(&mut ui, arrow(1.0, 0.0));
    assert_eq!(ui.focus(), ui.tab_button(tabs, 1));
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Changed(tabs)]);
    assert!(ui.is_shown(pages[1]));
}

#[test]
fn tabs_are_a_tab_list_with_selected_tabs_and_panels() {
    let (ui, tabs, pages) = settings();
    let tree = ui.accessibility_tree();
    let bar = tree.get(ui.parent(ui.tab_button(tabs, 0).unwrap()).unwrap()).unwrap();
    assert_eq!(bar.role, UiRole::TabList);
    let first = tree.get(ui.tab_button(tabs, 0).unwrap()).unwrap();
    assert_eq!(
        (first.role, first.selected, first.position),
        (UiRole::Tab, true, Some((1, 3)))
    );
    assert!(!tree.get(ui.tab_button(tabs, 1).unwrap()).unwrap().selected);
    assert_eq!(tree.get(pages[0]).unwrap().role, UiRole::TabPanel);
    assert!(tree.get(pages[1]).is_none(), "hidden pages are left out");
}

// ---- radio group and checkbox ---------------------------------------------------

#[test]
fn a_radio_group_keeps_one_button_checked() {
    let (mut ui, root) = screen();
    let group = ui.radio_group(root).selected(0).id();
    let radios = ["Off", "Low", "High"].map(|t| ui.radio(group, t).layout(UiLayout::fixed(120.0, 28.0)).id());
    idle(&mut ui);
    assert_eq!(ui.value(radios[0]), Some(1.0));
    assert_eq!(click_on(&mut ui, radios[2]), [UiEvent::Changed(group)]);
    assert_eq!(ui.selected(group), Some(2));
    assert_eq!(radios.map(|r| ui.value(r).unwrap()), [0.0, 0.0, 1.0]);
    // Keyboard and gamepad: up, then confirm.
    step(&mut ui, arrow(0.0, -1.0));
    assert_eq!(ui.focus(), Some(radios[1]));
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Changed(group)]);
    let tree = ui.accessibility_tree();
    assert_eq!(tree.get(group).unwrap().role, UiRole::RadioGroup);
    let low = tree.get(radios[1]).unwrap();
    assert_eq!(
        (low.role, &low.value, low.position),
        (UiRole::RadioButton, &UiAccessValue::Checked(true), Some((2, 3)))
    );
}

#[test]
fn a_checkbox_toggles_by_pointer_keyboard_and_gamepad() {
    let (mut ui, root) = screen();
    let music = ui.toggle(root, "Music").layout(UiLayout::fixed(120.0, 28.0)).id();
    idle(&mut ui);
    assert_eq!(click_on(&mut ui, music), [UiEvent::Changed(music)]);
    assert_eq!(ui.value(music), Some(1.0));
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Changed(music)]);
    assert_eq!(ui.value(music), Some(0.0));
    assert_eq!(ui.accessibility_tree().get(music).unwrap().role, UiRole::CheckBox);
}

// ---- progress bar and spinner ---------------------------------------------------

#[test]
fn progress_bars_and_spinners_take_no_input_and_say_what_they_show() {
    let (mut ui, root) = screen();
    let bar = ui
        .progress_bar(root)
        .layout(UiLayout::fixed(200.0, 12.0))
        .value(0.4)
        .accessible_name("Loading")
        .id();
    let busy = ui.spinner(root).accessible_name("Saving").id();
    idle(&mut ui);
    assert_eq!(ui.rect(busy).unwrap().size(), vec2(24.0, 24.0));
    let tree = ui.accessibility_tree();
    let (b, s) = (tree.get(bar).unwrap(), tree.get(busy).unwrap());
    assert_eq!(
        (b.role, &b.value, b.focusable),
        (UiRole::ProgressBar, &UiAccessValue::Range(0.4), false)
    );
    assert_eq!(
        (s.role, &s.value, s.focusable),
        (UiRole::Spinner, &UiAccessValue::None, false)
    );
    assert!(click_on(&mut ui, bar).is_empty());
    step(
        &mut ui,
        UiInput {
            next: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.focus(), None, "nothing to focus");
}

// ---- number field ---------------------------------------------------------------

fn amount() -> (Ui, UiId) {
    let (mut ui, root) = screen();
    let field = ui
        .number_field(root, 5.0)
        .range(1.0, 10.0)
        .layout(UiLayout::fixed(120.0, 32.0))
        .accessible_name("Amount")
        .id();
    idle(&mut ui);
    ui.set_focus(field);
    (ui, field)
}

#[test]
fn a_number_field_steps_with_the_arrows_and_stops_at_its_ends() {
    let (mut ui, field) = amount();
    assert_eq!(step(&mut ui, arrow(1.0, 0.0)), [UiEvent::Changed(field)]);
    assert_eq!((ui.number(field), ui.text(field)), (Some(6.0), Some("6")));
    for _ in 0..10 {
        step(&mut ui, arrow(1.0, 0.0));
    }
    assert_eq!(ui.number(field), Some(10.0));
    assert!(step(&mut ui, arrow(1.0, 0.0)).is_empty(), "no change at the top");
    step(&mut ui, arrow(-1.0, 0.0));
    assert_eq!(ui.number(field), Some(9.0));
}

#[test]
fn a_number_fields_ends_step_it_with_the_pointer() {
    let (mut ui, field) = amount();
    let r = ui.rect(field).unwrap();
    click(&mut ui, vec2(r.max.x - 5.0, r.center().y));
    assert_eq!(ui.number(field), Some(6.0));
    click(&mut ui, vec2(r.min.x + 5.0, r.center().y));
    click(&mut ui, vec2(r.min.x + 5.0, r.center().y));
    assert_eq!(ui.number(field), Some(4.0));
    assert!(click(&mut ui, r.center()).is_empty(), "the middle doesn't step");
}

#[test]
fn a_number_field_takes_typed_numbers_and_escape_drops_them() {
    let (mut ui, field) = amount();
    let root = ui.top_screen().unwrap();
    step(
        &mut ui,
        UiInput {
            text: "7".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        (ui.text(field), ui.number(field)),
        (Some("7"), Some(5.0)),
        "typed, not kept yet"
    );
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Changed(field)]);
    assert_eq!(ui.number(field), Some(7.0));
    // Out of range is kept in range.
    step(
        &mut ui,
        UiInput {
            text: "42".into(),
            ..Default::default()
        },
    );
    step(&mut ui, confirm());
    assert_eq!(ui.number(field), Some(10.0));
    // Escape drops what was typed and leaves the screen open.
    step(
        &mut ui,
        UiInput {
            text: "3".into(),
            ..Default::default()
        },
    );
    let events = step(&mut ui, cancel());
    assert!(events.is_empty(), "{events:?}");
    assert_eq!((ui.text(field), ui.top_screen()), (Some("10"), Some(root)));
    // Moving on keeps it.
    step(
        &mut ui,
        UiInput {
            text: "2".into(),
            ..Default::default()
        },
    );
    ui.clear_focus();
    assert_eq!(idle(&mut ui), [UiEvent::Changed(field)]);
    assert_eq!(ui.number(field), Some(2.0));
}

#[test]
fn a_number_field_is_a_spin_button() {
    let (mut ui, field) = amount();
    let node = ui.accessibility_tree().get(field).unwrap().clone();
    assert_eq!(node.role, UiRole::SpinButton);
    assert_eq!(
        node.value,
        UiAccessValue::Number {
            value: 5.0,
            min: 1.0,
            max: 10.0,
            step: 1.0
        }
    );
    ui.push_access_action(UiAccessAction::Increment(field));
    idle(&mut ui);
    ui.push_access_action(UiAccessAction::SetValue(field, 2.0));
    assert_eq!(idle(&mut ui), [UiEvent::Changed(field)]);
    assert_eq!(ui.number(field), Some(2.0));
}

// ---- list and grid ----------------------------------------------------------------

fn saves(len: usize) -> (Ui, UiId) {
    let (mut ui, root) = screen();
    let list = ui
        .list(root, 24.0)
        .layout(UiLayout::fixed(300.0, 240.0))
        .accessible_name("Saves")
        .id();
    ui.set_items(list, (0..len).map(|i| format!("Save {i}")));
    idle(&mut ui);
    (ui, list)
}

/// How many nodes a UI holds: walks every screen.
fn node_count(ui: &Ui) -> usize {
    ui.accessibility_tree().nodes().len()
}

#[test]
fn a_list_keeps_the_same_rows_whatever_its_length() {
    let counts: Vec<usize> = [20, 10_000, 1_000_000]
        .into_iter()
        .map(|len| {
            let (ui, list) = saves(len);
            assert_eq!(ui.list_len(list), Some(len));
            // 240 / 24 = 10 rows on screen, plus one scrolling in.
            assert_eq!(ui.list_rows(list).count(), 11);
            node_count(&ui)
        })
        .collect();
    assert_eq!(counts[0], counts[1]);
    assert_eq!(counts[1], counts[2]);
}

#[test]
fn a_list_scrolls_its_rows_over_ten_thousand_items() {
    let (mut ui, list) = saves(10_000);
    let rows_before: Vec<UiId> = ui.list_rows(list).map(|(row, _)| row).collect();
    let middle = center(&ui, list);
    let events = step(
        &mut ui,
        UiInput {
            pointer: middle,
            wheel: vec2(0.0, 24.0 * 5000.0),
            ..Default::default()
        },
    );
    assert!(events.contains(&UiEvent::RowsShown(list)), "{events:?}");
    let shown: Vec<(UiId, usize)> = ui.list_rows(list).collect();
    assert_eq!(shown.first().map(|s| s.1), Some(5000));
    // The same row nodes, reused.
    let mut rows: Vec<UiId> = shown.iter().map(|s| s.0).collect();
    rows.sort_by_key(|r| r.bits());
    let mut before = rows_before.clone();
    before.sort_by_key(|r| r.bits());
    assert_eq!(rows, before);
    // Each row shows its item's text where the item is.
    let (row, item) = shown[3];
    assert_eq!(ui.text(row), Some(format!("Save {item}").as_str()));
    let top = ui.rect(list).unwrap().min.y;
    assert!((ui.rect(row).unwrap().min.y - (top + 3.0 * 24.0)).abs() < 0.5);
    // The end: scrolled no further than the last item.
    step(
        &mut ui,
        UiInput {
            pointer: middle,
            wheel: vec2(0.0, 1.0e9),
            ..Default::default()
        },
    );
    assert_eq!(ui.list_rows(list).last().map(|s| s.1), Some(9_999));
}

#[test]
fn a_list_chooses_by_click_and_by_arrows_that_scroll_to_the_choice() {
    let (mut ui, list) = saves(10_000);
    let (row, item) = ui.list_rows(list).nth(2).unwrap();
    assert_eq!(click_on(&mut ui, row), [UiEvent::Changed(list)]);
    assert_eq!((ui.selected(list), ui.focus()), (Some(item), Some(list)));
    for _ in 0..20 {
        step(&mut ui, arrow(0.0, 1.0));
    }
    assert_eq!(ui.selected(list), Some(22));
    assert!(ui.list_rows(list).any(|(_, i)| i == 22), "scrolled to the choice");
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Submitted(list)]);
    // At the top, up leaves the list.
    ui.set_selected(list, Some(0));
    step(&mut ui, arrow(0.0, -1.0));
    assert_eq!(ui.selected(list), Some(0));
}

#[test]
fn a_grid_moves_by_cells_and_rows() {
    let (mut ui, root) = screen();
    let bag = ui.grid_view(root, 4, 64.0).layout(UiLayout::fixed(256.0, 192.0)).id();
    ui.set_list_len(bag, 400);
    idle(&mut ui);
    ui.set_focus(bag);
    ui.set_selected(bag, Some(5));
    step(&mut ui, arrow(1.0, 0.0));
    assert_eq!(ui.selected(bag), Some(6));
    step(&mut ui, arrow(0.0, 1.0));
    assert_eq!(ui.selected(bag), Some(10));
    // The cells sit on a 4-wide grid.
    let (row, _) = ui.list_rows(bag).nth(5).unwrap();
    let r = ui.rect(row).unwrap();
    let top = ui.rect(bag).unwrap().min;
    assert_eq!((r.min - top).round(), vec2(r.size().x, 64.0).round());
    let tree = ui.accessibility_tree();
    assert_eq!(tree.get(bag).unwrap().role, UiRole::Grid);
    let cell = tree.get(row).unwrap();
    assert_eq!((cell.role, cell.position), (UiRole::GridCell, Some((6, 400))));
}

#[test]
fn list_rows_are_list_items_numbered_among_all_items() {
    let (mut ui, list) = saves(10_000);
    ui.set_selected(list, Some(1));
    idle(&mut ui);
    let tree = ui.accessibility_tree();
    let node = tree.get(list).unwrap();
    assert_eq!((node.role, node.name.as_str()), (UiRole::List, "Saves"));
    let second = tree.get(node.children[1]).unwrap();
    assert_eq!(
        (second.role, second.name.as_str(), second.position, second.selected),
        (UiRole::ListItem, "Save 1", Some((2, 10_000)), true)
    );
}

// ---- modal dialog -----------------------------------------------------------------

#[test]
fn a_modal_dialog_traps_the_focus_and_gives_it_back() {
    let (mut ui, root) = screen();
    let open = ui.button(root, "Quit").layout(UiLayout::fixed(120.0, 32.0)).id();
    let other = ui.button(root, "Other").layout(UiLayout::fixed(120.0, 32.0)).id();
    idle(&mut ui);
    ui.set_focus(open);
    let dialog = ui
        .root()
        .layout(UiLayout::centered(300.0, 140.0).padding(10.0).gap(6.0))
        .id();
    ui.label(dialog, "Quit the game?");
    let yes = ui
        .button(dialog, "Yes")
        .layout(UiLayout::fixed(100.0, 30.0))
        .focused()
        .id();
    let no = ui
        .button(dialog, "No")
        .layout(UiLayout::fixed(100.0, 30.0))
        .closes_screen()
        .id();
    ui.push_modal(dialog);
    idle(&mut ui);
    assert_eq!(ui.focus(), Some(yes));
    // Tab (and the d-pad) only cycle inside the dialog.
    let tab = UiInput {
        next: true,
        ..Default::default()
    };
    let mut seen = Vec::new();
    for _ in 0..6 {
        step(&mut ui, tab.clone());
        seen.push(ui.focus().unwrap());
    }
    assert!(seen.iter().all(|&f| f == yes || f == no), "{seen:?}");
    step(&mut ui, arrow(0.0, 1.0));
    assert!(matches!(ui.focus(), Some(f) if f == yes || f == no));
    // A click outside it is swallowed.
    assert!(click_on(&mut ui, other).is_empty());
    let tree = ui.accessibility_tree();
    assert_eq!(tree.get(dialog).unwrap().role, UiRole::Dialog);
    assert!(tree.get(open).is_none(), "what is under a modal is hidden");
    // Escape (a gamepad's East button) closes it; the focus comes back.
    assert_eq!(step(&mut ui, cancel()), [UiEvent::ScreenPopped(dialog)]);
    assert_eq!(ui.focus(), Some(open));
}

// ---- context menu -----------------------------------------------------------------

fn potion() -> (Ui, UiId) {
    let (mut ui, root) = screen();
    let slot = ui
        .button(root, "Potion")
        .layout(UiLayout::fixed(100.0, 40.0))
        .context_menu(["Use", "Split", "Drop"])
        .id();
    idle(&mut ui);
    (ui, slot)
}

#[test]
fn the_right_button_opens_a_context_menu_at_the_pointer() {
    let (mut ui, slot) = potion();
    let at = center(&ui, slot);
    step(
        &mut ui,
        UiInput {
            pointer: at,
            secondary: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.popup_owner(), Some(slot));
    let menu = ui.top_screen().unwrap();
    assert!((ui.rect(menu).unwrap().min - at).length() < 1.0, "at the pointer");
    let drop = ui.accessibility_tree().get(menu).unwrap().children[2];
    let events = click_on(&mut ui, drop);
    assert_eq!(events, [UiEvent::Chosen(slot, 2)]);
    assert_eq!(ui.popup_owner(), None);
}

#[test]
fn the_menu_input_opens_the_focused_nodes_context_menu() {
    let (mut ui, slot) = potion();
    ui.set_focus(slot);
    // Shift+F10, or a gamepad's North button.
    step(
        &mut ui,
        UiInput {
            menu: true,
            ..Default::default()
        },
    );
    let menu = ui.top_screen().unwrap();
    let tree = ui.accessibility_tree();
    assert_eq!(tree.get(menu).unwrap().role, UiRole::Menu);
    let first = tree.get(ui.focus().unwrap()).unwrap();
    assert_eq!(
        (first.role, first.name.as_str(), first.position),
        (UiRole::MenuItem, "Use", Some((1, 3)))
    );
    step(&mut ui, arrow(0.0, 1.0));
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Chosen(slot, 1)]);
    assert_eq!(ui.focus(), Some(slot));
    // Escape closes one without choosing.
    step(
        &mut ui,
        UiInput {
            menu: true,
            ..Default::default()
        },
    );
    assert!(step(&mut ui, cancel()).is_empty());
    assert_eq!(ui.popup_owner(), None);
}

#[test]
fn removing_a_menus_owner_closes_its_menu() {
    let (mut ui, slot) = potion();
    ui.open_context_menu(slot);
    let menu = ui.top_screen().unwrap();
    ui.remove(slot);
    assert_eq!(ui.popup_owner(), None);
    assert!(ui.rect(menu).is_none(), "the menu's nodes are gone");
}

// ---- split panes ---------------------------------------------------------------

#[test]
fn dragging_a_split_divider_resizes_its_panes() {
    let (mut ui, root) = screen();
    let split = ui.split(root, UiAxis::Horizontal).layout(UiLayout::fill()).id();
    let (left, right) = ui.panes(split).unwrap();
    idle(&mut ui);
    let whole = ui.rect(split).unwrap();
    assert!((ui.rect(left).unwrap().size().x - (whole.size().x - 6.0) * 0.5).abs() < 4.0);
    let divider = ui.accessibility_tree().get(split).unwrap().children[1];
    let grab = center(&ui, divider);
    step(
        &mut ui,
        UiInput {
            pointer: grab,
            pressed: true,
            held: true,
            ..Default::default()
        },
    );
    let to = vec2(whole.min.x + whole.size().x * 0.25, grab.y);
    let events = step(
        &mut ui,
        UiInput {
            pointer: to,
            held: true,
            ..Default::default()
        },
    );
    assert_eq!(events, [UiEvent::Changed(split)]);
    step(
        &mut ui,
        UiInput {
            pointer: to,
            released: true,
            ..Default::default()
        },
    );
    assert!((ui.value(split).unwrap() - 0.25).abs() < 0.01);
    assert!(ui.rect(left).unwrap().size().x < ui.rect(right).unwrap().size().x / 2.0);
    // The keyboard moves it too, as a splitter with a value.
    ui.set_focus(divider);
    step(&mut ui, arrow(1.0, 0.0));
    assert!((ui.value(split).unwrap() - 0.30).abs() < 0.01);
    let node = ui.accessibility_tree().get(divider).unwrap().clone();
    assert_eq!(node.role, UiRole::Splitter);
    assert!(matches!(node.value, UiAccessValue::Range(v) if (v - 0.30).abs() < 0.01));
}

#[test]
fn a_vertical_split_stacks_its_panes() {
    let (mut ui, root) = screen();
    let split = ui
        .split(root, UiAxis::Vertical)
        .layout(UiLayout::fill())
        .value(0.75)
        .id();
    let (top, bottom) = ui.panes(split).unwrap();
    idle(&mut ui);
    let (t, b) = (ui.rect(top).unwrap(), ui.rect(bottom).unwrap());
    assert!(t.max.y <= b.min.y && t.size().y > 2.0 * b.size().y, "{t:?} {b:?}");
    let divider = ui.accessibility_tree().get(split).unwrap().children[1];
    ui.set_focus(divider);
    step(&mut ui, arrow(0.0, -1.0));
    assert!((ui.value(split).unwrap() - 0.70).abs() < 0.01);
}

// ---- drag and drop ---------------------------------------------------------------

fn inventory() -> (Ui, UiId, UiId, UiId) {
    let (mut ui, root) = screen();
    let row = ui.panel(root).layout(UiLayout::row(120.0).gap(20.0)).id();
    let bag = ui.panel(row).layout(UiLayout::fixed(150.0, 100.0)).drop_target().id();
    let chest = ui.panel(row).layout(UiLayout::fixed(150.0, 100.0)).drop_target().id();
    let gem = ui
        .button(bag, "Gem")
        .layout(UiLayout::fixed(60.0, 40.0))
        .draggable()
        .id();
    idle(&mut ui);
    (ui, bag, chest, gem)
}

#[test]
fn dragging_an_item_onto_another_container_drops_it_there() {
    let (mut ui, bag, chest, gem) = inventory();
    let from = center(&ui, gem);
    let to = center(&ui, chest);
    step(
        &mut ui,
        UiInput {
            pointer: from,
            pressed: true,
            held: true,
            ..Default::default()
        },
    );
    step(
        &mut ui,
        UiInput {
            pointer: (from + to) / 2.0,
            held: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.dragging(), Some(gem));
    let events = step(
        &mut ui,
        UiInput {
            pointer: to,
            released: true,
            ..Default::default()
        },
    );
    assert_eq!(events, [UiEvent::Dropped(gem, chest)], "a drop, not a click");
    assert_eq!(ui.dragging(), None);
    // The game accepts it.
    assert!(ui.set_parent(gem, chest));
    idle(&mut ui);
    assert!(ui.rect(chest).unwrap().contains(center(&ui, gem)));
    assert_eq!(ui.parent(gem), Some(chest));
    assert!(!ui.set_parent(bag, bag), "not into itself");
}

#[test]
fn a_click_without_moving_still_presses_a_draggable_button() {
    let (mut ui, _, _, gem) = inventory();
    assert_eq!(click_on(&mut ui, gem), [UiEvent::Clicked(gem)]);
}

#[test]
fn an_item_is_carried_and_dropped_with_the_keyboard_and_a_gamepad() {
    let (mut ui, bag, chest, gem) = inventory();
    ui.set_focus(gem);
    assert!(step(&mut ui, confirm()).is_empty(), "picked up, not pressed");
    assert_eq!(ui.dragging(), Some(gem));
    // Drop targets take focus while an item is carried: first the bag it
    // is in, then the chest.
    step(&mut ui, arrow(1.0, 0.0));
    assert_eq!(ui.focus(), Some(bag));
    step(&mut ui, arrow(1.0, 0.0));
    assert_eq!(ui.focus(), Some(chest));
    assert_eq!(step(&mut ui, confirm()), [UiEvent::Dropped(gem, chest)]);
    // Escape puts a carried item back.
    ui.set_focus(gem);
    step(&mut ui, confirm());
    let root = ui.top_screen();
    assert!(step(&mut ui, cancel()).is_empty());
    assert_eq!((ui.dragging(), ui.top_screen()), (None, root));
    // Assistive technology activates the same way.
    ui.push_access_action(UiAccessAction::Activate(gem));
    idle(&mut ui);
    ui.push_access_action(UiAccessAction::Activate(chest));
    assert_eq!(idle(&mut ui), [UiEvent::Dropped(gem, chest)]);
}

// ---- key binding ------------------------------------------------------------------

#[test]
fn a_key_binding_field_records_the_next_key_mouse_or_gamepad_button() {
    let (mut ui, root) = screen();
    let jump = ui
        .key_binding(root, Some(ButtonBinding::Key(Key::Space)))
        .layout(UiLayout::fixed(140.0, 32.0))
        .accessible_name("Jump")
        .id();
    let other = ui.button(root, "Other").layout(UiLayout::fixed(140.0, 32.0)).id();
    idle(&mut ui);
    assert_eq!(click_on(&mut ui, jump), []);
    assert!(ui.is_capturing(jump));
    // While it listens, nothing else takes input.
    assert!(step(&mut ui, arrow(0.0, 1.0)).is_empty());
    assert_eq!(ui.focus(), Some(jump));
    let pad = ButtonBinding::Gamepad {
        pad: Gamepad::from_slot(0).unwrap(),
        button: GamepadButton::West,
    };
    let events = step(
        &mut ui,
        UiInput {
            captured: Some(pad.clone()),
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(events, [UiEvent::Changed(jump)]);
    assert_eq!(
        (ui.binding(jump), ui.text(jump), ui.is_capturing(jump)),
        (Some(&pad), Some("Pad West"), false)
    );
    // Escape stops listening and keeps the binding; the screen stays.
    step(&mut ui, confirm());
    let screen_root = ui.top_screen();
    let escape = UiInput {
        cancel: true,
        captured: Some(ButtonBinding::Key(Key::Escape)),
        ..Default::default()
    };
    assert!(step(&mut ui, escape).is_empty());
    assert_eq!((ui.binding(jump), ui.top_screen()), (Some(&pad), screen_root));
    // A mouse button binds too.
    step(&mut ui, confirm());
    let right = ButtonBinding::Mouse(MouseButton::Right);
    step(
        &mut ui,
        UiInput {
            captured: Some(right.clone()),
            ..Default::default()
        },
    );
    assert_eq!(ui.binding(jump), Some(&right));
    let node = ui.accessibility_tree().get(jump).unwrap().clone();
    assert_eq!(
        (node.role, node.name.as_str(), node.value),
        (UiRole::Button, "Jump", UiAccessValue::Text("Mouse Right".into()))
    );
    let _ = other;
}

// ---- tooltip -------------------------------------------------------------------

#[test]
fn a_tooltip_shows_after_the_pointer_rests_and_at_once_for_the_keyboard() {
    let (mut ui, root) = screen();
    let vsync = ui
        .toggle(root, "V-sync")
        .layout(UiLayout::fixed(120.0, 30.0))
        .tooltip("Waits for the display")
        .id();
    let other = ui.button(root, "Other").layout(UiLayout::fixed(120.0, 30.0)).id();
    idle(&mut ui);
    let over = UiInput {
        pointer: center(&ui, vsync),
        dt: 0.2,
        ..Default::default()
    };
    // Rested from the frame it arrives: 0.0, 0.2, 0.4 s.
    for _ in 0..3 {
        step(&mut ui, over.clone());
    }
    assert!(ui.shown_tooltip().is_none(), "0.4 s is not long enough");
    step(&mut ui, over.clone());
    let (whose, rect) = ui.shown_tooltip().unwrap();
    assert_eq!(whose, vsync);
    assert!(
        rect.min.y > over.pointer.y && rect.size().x > 50.0,
        "below the pointer: {rect:?}"
    );
    // A press hides it; leaving starts over.
    step(
        &mut ui,
        UiInput {
            pressed: true,
            held: true,
            ..over.clone()
        },
    );
    assert!(ui.shown_tooltip().is_none());
    step(
        &mut ui,
        UiInput {
            released: true,
            ..over.clone()
        },
    );
    let away = center(&ui, other);
    step(
        &mut ui,
        UiInput {
            pointer: away,
            dt: 1.0,
            ..Default::default()
        },
    );
    assert!(ui.shown_tooltip().is_none());
    // The keyboard (and the d-pad) show it at once, under the node.
    ui.set_focus(other);
    step(
        &mut ui,
        UiInput {
            pointer: vec2(-50.0, -50.0),
            direction: vec2(0.0, -1.0),
            ..Default::default()
        },
    );
    assert_eq!(ui.focus(), Some(vsync));
    let (whose, rect) = ui.shown_tooltip().unwrap();
    assert_eq!(whose, vsync);
    assert!(rect.min.y >= ui.rect(vsync).unwrap().max.y);
    // Screen readers read it as the description.
    let node = ui.accessibility_tree().get(vsync).unwrap().clone();
    assert_eq!(node.description, "Waits for the display");
}

// ---- theme ----------------------------------------------------------------------

#[test]
fn the_light_and_dark_themes_set_every_token_and_reach_new_widgets() {
    let (light, dark) = (UiTheme::light(), UiTheme::dark());
    assert!(light.panel.r > 0.8 && dark.panel.r < 0.2);
    assert!(light.text.r < 0.2 && dark.text.r > 0.8);
    assert_eq!((light.radius, dark.radius), (4.0, 4.0));
    assert_eq!(light.spacing, 8.0);
    let (mut ui, root) = screen();
    let dropdown = ui.dropdown(root, ["A"]).id();
    ui.set_theme(light);
    assert_eq!(ui.paint(dropdown).unwrap().background, light.control);
    assert_eq!(ui.paint(dropdown).unwrap().color, light.text);
    // A custom theme: the accent of your game, roomier spacing.
    let custom = UiTheme {
        accent: Color::hex(0x2e8b57),
        spacing: 12.0,
        ..UiTheme::dark()
    };
    ui.set_theme(custom);
    assert_eq!(ui.paint(dropdown).unwrap().border, custom.accent);
    // The spacing token spaces the pop-ups: each option is a text line and
    // the spacing tall.
    ui.set_layout(dropdown, UiLayout::fixed(120.0, 30.0));
    idle(&mut ui);
    ui.push_access_action(UiAccessAction::Activate(dropdown));
    idle(&mut ui);
    let option = ui.focus().unwrap();
    assert_eq!(ui.rect(option).unwrap().size().y, custom.text_size + 12.0);
}

#[test]
fn every_widget_has_its_role() {
    let (mut ui, root) = screen();
    let w = [
        ui.dropdown(root, ["A"]).id(),
        ui.tabs(root).id(),
        ui.radio_group(root).id(),
        ui.progress_bar(root).id(),
        ui.spinner(root).id(),
        ui.number_field(root, 1.0).id(),
        ui.list(root, 20.0).id(),
        ui.grid_view(root, 3, 20.0).id(),
        ui.split(root, UiAxis::Horizontal).id(),
        ui.key_binding(root, None).id(),
        ui.toggle(root, "C").id(),
    ];
    idle(&mut ui);
    let tree = ui.accessibility_tree();
    let roles: Vec<UiRole> = w.iter().map(|&id| tree.get(id).unwrap().role).collect();
    assert_eq!(
        roles,
        [
            UiRole::ComboBox,
            UiRole::Group,
            UiRole::RadioGroup,
            UiRole::ProgressBar,
            UiRole::Spinner,
            UiRole::SpinButton,
            UiRole::List,
            UiRole::Grid,
            UiRole::Group,
            UiRole::Button,
            UiRole::CheckBox,
        ]
    );
    for role in roles {
        assert!(!role.as_str().is_empty());
    }
    assert_eq!(UiRole::of(UiWidget::Splitter), UiRole::Splitter);
}

// ---- through the engine's input ----------------------------------------------------

#[test]
fn the_engines_input_opens_context_menus_and_binds_keys() {
    use ponko::actions::InputEvent;
    use ponko::scenario::InputScenario;
    use std::{cell::RefCell, rc::Rc};
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    let scenario = InputScenario::new()
        // The right button over the slot opens its menu; Escape closes it.
        .at(0, InputEvent::MouseMove(vec2(50.0, 20.0)))
        .at(0, InputEvent::Mouse(MouseButton::Right, true))
        .at(1, InputEvent::Mouse(MouseButton::Right, false))
        .at(2, InputEvent::Key(Key::Escape, true))
        .at(3, InputEvent::Key(Key::Escape, false))
        // A click on the binding field listens; the next key binds.
        .at(4, InputEvent::MouseMove(vec2(50.0, 60.0)))
        .at(4, InputEvent::Mouse(MouseButton::Left, true))
        .at(5, InputEvent::Mouse(MouseButton::Left, false))
        .at(6, InputEvent::Key(Key::K, true))
        .at(7, InputEvent::Key(Key::K, false))
        .duration(8);
    let (_, report) = Game::new("widgets", 640, 480)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let slot = ctx.ui.find("slot").unwrap();
            let jump = ctx.ui.find("jump").unwrap();
            while ctx.ui.next_event().is_some() {}
            seen.borrow_mut().push((
                ctx.ui.popup_owner() == Some(slot),
                ctx.ui.is_capturing(jump),
                ctx.ui.binding(jump).cloned(),
            ));
        })
        .run_headless_scenario(scenario, |ctx| {
            let root = ctx.ui.root().layout(UiLayout::fill()).id();
            ctx.ui
                .button(root, "Potion")
                .name("slot")
                .layout(UiLayout::fixed(100.0, 40.0))
                .context_menu(["Use"]);
            ctx.ui
                .key_binding(root, None)
                .name("jump")
                .layout(UiLayout::fixed(100.0, 40.0));
            ctx.ui.push_modeless(root);
        });
    report.assert_passed();
    let log = log.borrow();
    assert!(log[0].0, "the right button opened the menu");
    assert!(!log[2].0, "Escape closed it");
    assert!(log[5].1, "a click starts listening");
    assert_eq!(log[6], (false, false, Some(ButtonBinding::Key(Key::K))));
}

// ---- the guide's gallery ------------------------------------------------------

/// The guide's gallery, `examples/ui_widgets.rs`: its nodes and what it does
/// with an event, without its `main`.
#[allow(dead_code)]
#[path = "../examples/ui_widgets.rs"]
mod gallery;

#[test]
fn the_gallery_example_reacts_and_its_dialog_keeps_the_focus() {
    let mut ui = Ui::default();
    let g = gallery::build(&mut ui);
    idle(&mut ui);
    assert_eq!(ui.selected(g.quality), Some(1));

    // Choose "High" with the keys: open, down, confirm.
    ui.set_focus(g.quality);
    for input in [confirm(), arrow(0.0, 1.0), confirm()] {
        for event in step(&mut ui, input) {
            gallery::react(&mut ui, &g, event);
        }
    }
    assert_eq!(ui.text(g.status), Some("Quality: High"));

    // Reset opens the modal dialog; Tab cycles inside it and never leaves.
    for event in click_on(&mut ui, g.reset) {
        gallery::react(&mut ui, &g, event);
    }
    assert_eq!(ui.top_screen(), Some(g.dialog));
    for _ in 0..5 {
        step(
            &mut ui,
            UiInput {
                next: true,
                ..Default::default()
            },
        );
        let focus = ui.focus().expect("the dialog keeps a focus");
        let inside = std::iter::successors(Some(focus), |&n| ui.parent(n)).any(|n| n == g.dialog);
        assert!(inside, "focus left the dialog");
    }
    for event in click_on(&mut ui, g.confirm) {
        gallery::react(&mut ui, &g, event);
    }
    assert_ne!(ui.top_screen(), Some(g.dialog));
    assert_eq!((ui.selected(g.quality), ui.text(g.status)), (Some(1), Some("Reset")));
}

#[test]
fn the_custom_theme_changes_tokens_and_colors() {
    let custom = gallery::custom_theme();
    assert_eq!((custom.radius, custom.spacing), (0.0, 12.0));
    assert_ne!(custom.accent, UiTheme::dark().accent);
    assert_eq!(custom.panel, UiTheme::dark().panel);
    let mut ui = Ui::default();
    let g = gallery::build(&mut ui);
    ui.set_theme(custom);
    idle(&mut ui);
    assert_eq!(ui.theme().radius, 0.0);
    assert_eq!(ui.paint(g.reset).map(|p| p.border), Some(custom.accent));
}

#[test]
fn an_edit_menu_of_the_games_own_cuts_the_selection() {
    // A "Cut" entry in a context menu: copy what is selected, then cut it.
    let (mut ui, root) = screen();
    let field = ui
        .text_input(root)
        .text("hello")
        .layout(UiLayout::fixed(200.0, 30.0))
        .id();
    assert!(ui.set_focus(field));
    step(
        &mut ui,
        UiInput {
            select_all: true,
            ..Default::default()
        },
    );
    let copied = ui.selected_text().map(str::to_owned);
    ui.cut_selection();
    assert_eq!(copied.as_deref(), Some("hello"));
    assert_eq!(ui.text(field), Some(""));
    assert!(ui.events().any(|e| e == UiEvent::Changed(field)));
}

#[test]
fn a_screen_reports_how_it_was_built() {
    // What an editor or a test of the game's own menus reads back.
    let (mut ui, root) = screen();
    let tabs = ui.tabs(root).id();
    let audio = ui.tab(tabs, "Audio").id();
    let video = ui.tab(tabs, "Video").id();
    assert_eq!(
        (ui.tab_page(tabs, 0), ui.tab_page(tabs, 1), ui.tab_page(tabs, 2)),
        (Some(audio), Some(video), None)
    );
    let play = ui.button(root, "Play").focused().id();
    assert!(ui.is_initial_focus(play) && !ui.is_initial_focus(tabs));
    let card = ui.button(root, "Card").draggable().id();
    ui.set_drop_target(root, true);
    assert!(ui.is_draggable(card) && !ui.is_draggable(play));
    assert!(ui.is_drop_target(root) && !ui.is_drop_target(card));

    let half = UiLayout::row(30.0).height_fraction(0.5);
    assert!(!half.is_hidden() && half.hidden().is_hidden());
    let theme = UiTheme {
        title: Color::YELLOW,
        title_size: 30.0,
        ..Default::default()
    };
    assert_eq!(theme.title_paint().font_size, 30.0);
    // A key-binding row in a settings screen of the game's own drawing.
    assert_eq!(ponko::ui::binding_label(&ButtonBinding::Key(Key::Space)), "Space");
    assert_eq!(ponko::ui::binding_label(&ButtonBinding::Key(Key::Num1)), "1");
}
