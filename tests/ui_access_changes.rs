//! What changed in the UI's accessibility semantics, and what assistive
//! technology can ask of it: the change tracker, the requests, and the
//! richer semantics (descriptions, live regions, the text caret). No
//! platform adapter and no window.

use ponko::prelude::*;
use ponko::ui::{Ui, UiAccessAction, UiAccessTracker, UiEvent, UiId, UiInput, UiLayout, UiLive};

const VIEW: Vec2 = Vec2::new(800.0, 600.0);

struct Menu {
    ui: Ui,
    root: UiId,
    title: UiId,
    play: UiId,
    volume: UiId,
    name: UiId,
    music: UiId,
    quit: UiId,
}

/// The sample menu: a title, a button, a slider, a text input, a toggle and
/// a disabled button.
fn menu() -> Menu {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(4.0)).id();
    let title = ui.label(root, "Settings").layout(UiLayout::row(24.0)).id();
    let play = ui.button(root, "Play").layout(UiLayout::row(30.0)).id();
    let volume = ui
        .slider(root)
        .layout(UiLayout::row(20.0))
        .value(0.5)
        .accessible_name("Volume")
        .id();
    let name = ui
        .text_input(root)
        .layout(UiLayout::row(30.0))
        .text("Ann")
        .accessible_name("Player name")
        .id();
    let music = ui.toggle(root, "Music").layout(UiLayout::row(30.0)).value(1.0).id();
    let quit = ui.button(root, "Quit").layout(UiLayout::row(30.0)).enabled(false).id();
    ui.push_modeless(root);
    ui.layout(VIEW);
    Menu {
        ui,
        root,
        title,
        play,
        volume,
        name,
        music,
        quit,
    }
}

fn idle(ui: &mut Ui) {
    ui.frame(VIEW, &UiInput::default());
}

/// A tracker that has already seen the menu.
fn watched() -> (Menu, UiAccessTracker) {
    let m = menu();
    let mut tracker = UiAccessTracker::new();
    let first = tracker.update(&m.ui).expect("the first look reports");
    assert_eq!(first.changed.len(), 7);
    (m, tracker)
}

// ------------------------------------------------------------- the tracker

#[test]
fn the_first_look_reports_every_node_and_an_unchanged_ui_reports_nothing() {
    let m = menu();
    let mut tracker = UiAccessTracker::new();
    let first = tracker.update(&m.ui).unwrap();
    assert_eq!(
        first.changed,
        [m.root, m.title, m.play, m.volume, m.name, m.music, m.quit],
        "in tree order, a parent before its children"
    );
    assert!(first.removed.is_empty());
    assert!(first.screens_changed);
    assert_eq!(tracker.tree(), &m.ui.accessibility_tree());
    for _ in 0..3 {
        assert_eq!(tracker.update(&m.ui), None);
    }
    // An empty UI still has a first look, so an adapter can publish "nothing".
    let empty = UiAccessTracker::new().update(&Ui::default()).unwrap();
    assert!(empty.changed.is_empty() && empty.screens_changed);
}

#[test]
fn a_change_reports_only_the_nodes_it_touched() {
    let (mut m, mut tracker) = watched();

    m.ui.set_text(m.play, "Continue");
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.play], "a name");

    m.ui.set_value(m.volume, 0.75);
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.volume], "a slider value");

    m.ui.set_value(m.music, 0.0);
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.music], "a toggle");

    m.ui.set_text(m.name, "Bea");
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.name], "a text value");

    m.ui.set_accessible_name(m.volume, "Music volume");
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.volume], "a name override");

    m.ui.set_accessible_description(m.play, "Starts a new run");
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.play], "a description");

    m.ui.set_live_region(m.title, UiLive::Polite);
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.title], "a live region");

    // After every change the tracker's snapshot is the UI's.
    assert_eq!(tracker.tree(), &m.ui.accessibility_tree());
    assert_eq!(tracker.update(&m.ui), None);
}

#[test]
fn focus_moves_report_the_node_that_lost_it_and_the_one_that_got_it() {
    let (mut m, mut tracker) = watched();
    m.ui.frame(
        VIEW,
        &UiInput {
            next: true,
            ..Default::default()
        },
    );
    let to_play = tracker.update(&m.ui).unwrap();
    assert_eq!(to_play.changed, [m.play]);
    assert!(to_play.focus_changed && !to_play.screens_changed);
    assert_eq!(tracker.tree().focus(), Some(m.play));

    m.ui.frame(
        VIEW,
        &UiInput {
            next: true,
            ..Default::default()
        },
    );
    let to_volume = tracker.update(&m.ui).unwrap();
    assert_eq!(to_volume.changed, [m.play, m.volume]);
    assert!(to_volume.focus_changed);
}

#[test]
fn enabling_disabling_and_hiding_update_the_affected_subtree() {
    let (mut m, mut tracker) = watched();

    // Disabled: the button stays readable but can't be focused.
    m.ui.set_enabled(m.play, false);
    m.ui.layout(VIEW);
    let off = tracker.update(&m.ui).unwrap();
    assert_eq!((off.changed.as_slice(), off.removed.len()), (&[m.play][..], 0));
    let play = tracker.tree().get(m.play).unwrap();
    assert!(play.disabled && !play.focusable);

    m.ui.set_enabled(m.quit, true);
    m.ui.layout(VIEW);
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.quit]);
    assert!(tracker.tree().get(m.quit).unwrap().focusable);

    // Hidden: the node goes, and its parent's children change.
    m.ui.set_layout(m.volume, UiLayout::row(20.0).hidden());
    m.ui.layout(VIEW);
    let gone = tracker.update(&m.ui).unwrap();
    assert_eq!(gone.removed, [m.volume]);
    assert!(gone.changed.contains(&m.root), "the parent lists one child fewer");
    assert!(!gone.changed.contains(&m.volume));
    assert!(tracker.tree().get(m.volume).is_none());

    // Shown again: it is back, as a new node for the adapter.
    m.ui.set_layout(m.volume, UiLayout::row(20.0));
    m.ui.layout(VIEW);
    let back = tracker.update(&m.ui).unwrap();
    assert!(back.removed.is_empty());
    assert!(back.changed.contains(&m.volume) && back.changed.contains(&m.root));
    assert_eq!(tracker.tree(), &m.ui.accessibility_tree());
}

#[test]
fn adding_and_removing_nodes_is_reported_with_the_parent() {
    let (mut m, mut tracker) = watched();
    let credits = m.ui.button(m.root, "Credits").layout(UiLayout::row(30.0)).id();
    m.ui.layout(VIEW);
    let added = tracker.update(&m.ui).unwrap();
    assert!(added.changed.contains(&credits) && added.changed.contains(&m.root));
    assert!(added.removed.is_empty());

    m.ui.remove(credits);
    m.ui.layout(VIEW);
    let removed = tracker.update(&m.ui).unwrap();
    assert_eq!(removed.removed, [credits]);
    assert!(removed.changed.contains(&m.root));
    assert_eq!(tracker.update(&m.ui), None);
}

#[test]
fn modal_screens_remove_what_they_cover_and_popping_brings_it_back() {
    let (mut m, mut tracker) = watched();
    let dialog = m.ui.root().layout(UiLayout::centered(300.0, 120.0)).id();
    let yes = m.ui.button(dialog, "Yes").layout(UiLayout::row(30.0)).id();
    m.ui.push_modal(dialog);
    idle(&mut m.ui);

    let opened = tracker.update(&m.ui).unwrap();
    assert!(opened.screens_changed);
    assert_eq!(
        opened.removed,
        [m.root, m.title, m.play, m.volume, m.name, m.music, m.quit],
        "a modal screen hides everything below it"
    );
    assert_eq!(opened.changed, [dialog, yes]);
    assert_eq!(tracker.tree().screens(), [dialog]);
    assert!(tracker.tree().get(dialog).unwrap().modal);

    m.ui.pop_screen();
    idle(&mut m.ui);
    let closed = tracker.update(&m.ui).unwrap();
    assert!(closed.screens_changed);
    assert_eq!(closed.removed, [dialog, yes]);
    assert_eq!(closed.changed.len(), 7);
    assert_eq!(tracker.tree().screens(), [m.root]);
}

#[test]
fn a_non_modal_overlay_makes_the_screen_below_inert_without_removing_it() {
    let (mut m, mut tracker) = watched();
    let toast = m.ui.root().layout(UiLayout::anchored(0.0, 0.0, 200.0, 40.0)).id();
    m.ui.label(toast, "Saved").layout(UiLayout::row(20.0));
    m.ui.push_modeless(toast);
    idle(&mut m.ui);
    let shown = tracker.update(&m.ui).unwrap();
    assert!(shown.removed.is_empty() && shown.screens_changed);
    assert!(shown.changed.contains(&m.play), "it turned inert");
    let play = tracker.tree().get(m.play).unwrap();
    assert!(play.inert && !play.focusable);
}

#[test]
fn resetting_makes_the_next_look_report_everything_again() {
    let (m, mut tracker) = watched();
    tracker.reset();
    assert!(tracker.tree().nodes().is_empty());
    let again = tracker.update(&m.ui).unwrap();
    assert_eq!(again.changed.len(), 7);
    assert!(again.screens_changed);
}

#[test]
fn scaling_the_ui_moves_every_bound_and_nothing_else() {
    let (mut m, mut tracker) = watched();
    let before = tracker.tree().get(m.play).unwrap().clone();
    m.ui.set_scale(2.0);
    m.ui.layout(VIEW);
    let scaled = tracker.update(&m.ui).unwrap();
    assert!(scaled.changed.contains(&m.play) && scaled.removed.is_empty());
    assert!(!scaled.focus_changed && !scaled.screens_changed);
    let after = tracker.tree().get(m.play).unwrap();
    assert!((after.rect.size().y - before.rect.size().y * 2.0).abs() < 0.01);
    assert_eq!(
        (&after.name, after.role, after.focusable),
        (&before.name, before.role, true)
    );
}

// ---------------------------------------------------------------- requests

#[test]
fn a_request_does_what_the_matching_key_does_and_emits_the_same_events() {
    // Activate a button: the same event as Tab to it, then Enter.
    let mut by_key = menu();
    by_key.ui.frame(
        VIEW,
        &UiInput {
            next: true,
            ..Default::default()
        },
    );
    by_key.ui.frame(
        VIEW,
        &UiInput {
            confirm: true,
            ..Default::default()
        },
    );
    let key_events: Vec<UiEvent> = by_key.ui.events().collect();

    let mut by_request = menu();
    by_request
        .ui
        .push_access_action(UiAccessAction::Activate(by_request.play));
    idle(&mut by_request.ui);
    let request_events: Vec<UiEvent> = by_request.ui.events().collect();
    assert_eq!(key_events, [UiEvent::Clicked(by_key.play)]);
    assert_eq!(request_events, key_events);
    assert_eq!(by_request.ui.focus(), by_key.ui.focus());

    // Increment a slider: the same value as the right arrow.
    let mut by_key = menu();
    for input in [
        UiInput {
            next: true,
            ..Default::default()
        },
        UiInput {
            next: true,
            ..Default::default()
        },
        UiInput {
            direction: vec2(1.0, 0.0),
            ..Default::default()
        },
    ] {
        by_key.ui.frame(VIEW, &input);
    }
    let mut by_request = menu();
    by_request
        .ui
        .push_access_action(UiAccessAction::Increment(by_request.volume));
    idle(&mut by_request.ui);
    assert_eq!(by_request.ui.value(by_request.volume), by_key.ui.value(by_key.volume));
    assert_eq!(
        by_request.ui.events().collect::<Vec<_>>(),
        [UiEvent::Changed(by_request.volume)]
    );
}

#[test]
fn requests_focus_toggle_set_values_and_replace_text() {
    let mut m = menu();
    m.ui.push_access_action(UiAccessAction::Focus(m.name));
    idle(&mut m.ui);
    assert_eq!(m.ui.focus(), Some(m.name));
    assert_eq!(m.ui.events().count(), 0, "focusing emits nothing");

    m.ui.push_access_action(UiAccessAction::Activate(m.music));
    idle(&mut m.ui);
    assert_eq!(m.ui.value(m.music), Some(0.0));
    assert_eq!(m.ui.events().collect::<Vec<_>>(), [UiEvent::Changed(m.music)]);
    assert_eq!(m.ui.focus(), Some(m.music));

    m.ui.push_access_action(UiAccessAction::SetValue(m.music, 1.0));
    m.ui.push_access_action(UiAccessAction::SetValue(m.volume, 7.0));
    m.ui.push_access_action(UiAccessAction::Decrement(m.volume));
    idle(&mut m.ui);
    assert_eq!(m.ui.value(m.music), Some(1.0));
    assert!(
        (m.ui.value(m.volume).unwrap() - 0.95).abs() < 1e-6,
        "clamped to 1, then one step down"
    );
    assert_eq!(
        m.ui.events().collect::<Vec<_>>(),
        [
            UiEvent::Changed(m.music),
            UiEvent::Changed(m.volume),
            UiEvent::Changed(m.volume)
        ]
    );

    m.ui.push_access_action(UiAccessAction::SetText(m.name, "Zoë".into()));
    idle(&mut m.ui);
    assert_eq!(m.ui.text(m.name), Some("Zoë"));
    assert_eq!(m.ui.events().collect::<Vec<_>>(), [UiEvent::Changed(m.name)]);
    assert_eq!(
        m.ui.accessibility_tree().get(m.name).unwrap().selection,
        Some((4, 4)),
        "the caret is at the end, in bytes"
    );
    // The same text again changes nothing.
    m.ui.push_access_action(UiAccessAction::SetText(m.name, "Zoë".into()));
    idle(&mut m.ui);
    assert_eq!(m.ui.events().count(), 0);

    m.ui.push_access_action(UiAccessAction::Activate(m.name));
    idle(&mut m.ui);
    assert_eq!(m.ui.events().collect::<Vec<_>>(), [UiEvent::Submitted(m.name)]);
}

#[test]
fn requests_for_nodes_the_keyboard_could_not_reach_do_nothing() {
    let mut m = menu();
    // A label, a disabled button, a value of the wrong kind, a stale id.
    let stale = m.ui.button(m.root, "Gone").id();
    m.ui.remove(stale);
    for action in [
        UiAccessAction::Activate(m.title),
        UiAccessAction::Activate(m.quit),
        UiAccessAction::Focus(m.quit),
        UiAccessAction::Increment(m.play),
        UiAccessAction::SetText(m.play, "x".into()),
        UiAccessAction::SetValue(m.name, 1.0),
        UiAccessAction::SetValue(m.volume, f32::NAN),
        UiAccessAction::Activate(stale),
    ] {
        m.ui.push_access_action(action);
    }
    idle(&mut m.ui);
    assert_eq!(m.ui.events().count(), 0);
    assert_eq!(m.ui.focus(), None);
    assert_eq!((m.ui.text(m.play), m.ui.value(m.volume)), (Some("Play"), Some(0.5)));

    // Under a modal dialog the menu can't be reached either.
    let dialog = m.ui.root().layout(UiLayout::centered(300.0, 120.0)).id();
    let yes = m.ui.button(dialog, "Yes").layout(UiLayout::row(30.0)).id();
    m.ui.push_modal(dialog);
    idle(&mut m.ui);
    m.ui.push_access_action(UiAccessAction::Activate(m.play));
    m.ui.push_access_action(UiAccessAction::Activate(yes));
    idle(&mut m.ui);
    assert_eq!(m.ui.events().collect::<Vec<_>>(), [UiEvent::Clicked(yes)]);
}

#[test]
fn a_request_and_a_key_press_in_one_frame_both_apply_request_first() {
    let mut m = menu();
    m.ui.push_access_action(UiAccessAction::Focus(m.play));
    m.ui.frame(
        VIEW,
        &UiInput {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(m.ui.events().collect::<Vec<_>>(), [UiEvent::Clicked(m.play)]);
}

#[test]
fn focusing_a_node_scrolled_out_of_view_brings_it_into_view() {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill()).id();
    let list = ui.scroll_area(root).layout(UiLayout::fixed(300.0, 100.0)).id();
    let mut last = root;
    for i in 0..20 {
        // Rows that keep their height, so the list overflows.
        let mut row = UiLayout::row(30.0);
        row.unstable_style_mut().flex_shrink = 0.0;
        last = ui.button(list, format!("Item {i}")).layout(row).id();
    }
    ui.push_modeless(root);
    ui.layout(VIEW);
    assert_eq!(ui.scroll(list), Some(Vec2::ZERO));
    assert!(
        ui.rect(last).unwrap().min.y > ui.rect(list).unwrap().max.y,
        "the last item starts below the list"
    );
    ui.push_access_action(UiAccessAction::Focus(last));
    idle(&mut ui);
    assert_eq!(ui.focus(), Some(last));
    assert!(
        ui.scroll(list).unwrap().y > 0.0,
        "the list scrolled to the focused item"
    );
}

// ------------------------------------------------------- richer semantics

#[test]
fn descriptions_and_live_regions_are_in_the_tree_and_its_text() {
    let mut m = menu();
    m.ui.set_accessible_description(m.quit, "Closes the game");
    m.ui.set_live_region(m.title, UiLive::Assertive);
    let tree = m.ui.accessibility_tree();
    assert_eq!(tree.get(m.quit).unwrap().description, "Closes the game");
    assert_eq!(tree.get(m.title).unwrap().live, UiLive::Assertive);
    assert_eq!(tree.get(m.play).unwrap().live, UiLive::Off);
    let text = tree.to_string();
    assert!(text.contains("label \"Settings\" live=assertive\n"), "{text}");
    assert!(
        text.contains("button \"Quit\" description=\"Closes the game\" disabled\n"),
        "{text}"
    );
    // An empty description removes it; they change nothing else.
    m.ui.set_accessible_description(m.quit, "");
    assert_eq!(m.ui.accessible_description(m.quit), Some(""));
    assert_eq!(m.ui.text(m.quit), Some("Quit"));
    // Stale ids are ignored.
    m.ui.remove(m.quit);
    m.ui.set_accessible_description(m.quit, "x");
    m.ui.set_live_region(m.quit, UiLive::Polite);
    assert_eq!(m.ui.accessible_description(m.quit), None);
    assert_eq!(m.ui.live_region(m.quit), None);
}

#[test]
fn a_text_input_exposes_its_caret_and_selection() {
    let mut m = menu();
    let tree = m.ui.accessibility_tree();
    assert_eq!(
        tree.get(m.name).unwrap().selection,
        Some((3, 3)),
        "setting the text leaves the caret at its end"
    );
    assert_eq!(tree.get(m.play).unwrap().selection, None, "only text inputs have one");
    assert!(
        !tree.to_string().contains("caret="),
        "the text shows a caret only while editing"
    );

    // Focus it and move to the start.
    m.ui.push_access_action(UiAccessAction::Focus(m.name));
    m.ui.frame(
        VIEW,
        &UiInput {
            home: true,
            ..Default::default()
        },
    );
    let tree = m.ui.accessibility_tree();
    assert_eq!(tree.get(m.name).unwrap().selection, Some((0, 0)));
    assert!(
        tree.to_string()
            .contains("text-input \"Player name\" value=\"Ann\" caret=0 focusable focused\n"),
        "{tree}"
    );

    // Select everything: anchor at the start, caret at the end.
    m.ui.frame(
        VIEW,
        &UiInput {
            select_all: true,
            ..Default::default()
        },
    );
    let tree = m.ui.accessibility_tree();
    assert_eq!(tree.get(m.name).unwrap().selection, Some((0, 3)));
    assert!(tree.to_string().contains("selection=0..3"), "{tree}");

    // A tracker sees the caret move.
    let mut tracker = UiAccessTracker::new();
    tracker.update(&m.ui);
    m.ui.frame(
        VIEW,
        &UiInput {
            home: true,
            ..Default::default()
        },
    );
    assert_eq!(tracker.update(&m.ui).unwrap().changed, [m.name]);
}
