//! The accessibility change tracker costs no allocations while the UI's
//! semantics stay the same, so an adapter can ask every frame.

use ponko::prelude::*;
use ponko::ui::{Ui, UiAccessTracker, UiInput, UiLayout};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// Allocations made by this thread while it is counting.
    static COUNT: Cell<Option<u64>> = const { Cell::new(None) };
}

struct Counting;

// SAFETY: every call goes to the system allocator unchanged; the counter is
// a thread-local `Cell`, touched only through `try_with` so that a thread
// being torn down still allocates safely.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get().map(|n| n + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get().map(|n| n + 1)));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How many times this thread allocated while running `work`.
fn allocations(work: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(Some(0)));
    work();
    COUNT.with(|c| c.replace(None)).unwrap()
}

const VIEW: Vec2 = Vec2::new(800.0, 600.0);

#[test]
fn an_unchanged_ui_costs_the_tracker_no_allocations_and_a_change_is_still_seen() {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(4.0)).id();
    ui.label(root, "Settings").layout(UiLayout::row(24.0));
    let mut buttons = Vec::new();
    for i in 0..200 {
        buttons.push(ui.button(root, format!("Item {i}")).layout(UiLayout::row(30.0)).id());
    }
    let volume = ui
        .slider(root)
        .layout(UiLayout::row(20.0))
        .value(0.5)
        .accessible_name("Volume")
        .id();
    ui.text_input(root)
        .layout(UiLayout::row(30.0))
        .text("Ann")
        .accessible_name("Player name");
    ui.push_modeless(root);
    ui.frame(VIEW, &UiInput::default());

    let mut tracker = UiAccessTracker::new();
    let first = allocations(|| assert!(tracker.update(&ui).is_some()));
    assert!(first > 0, "the counter works: the first look builds a snapshot");

    // Steady state: frames pass, nothing about the semantics changes.
    let quiet = allocations(|| {
        for _ in 0..100 {
            assert!(tracker.update(&ui).is_none());
        }
    });
    assert_eq!(quiet, 0, "an unchanged UI must cost the tracker nothing");

    // It still notices the next change, and only that node.
    ui.set_value(volume, 0.75);
    assert_eq!(tracker.update(&ui).unwrap().changed, [volume]);
    ui.set_text(buttons[7], "Renamed");
    assert_eq!(tracker.update(&ui).unwrap().changed, [buttons[7]]);
    let quiet = allocations(|| assert!(tracker.update(&ui).is_none()));
    assert_eq!(quiet, 0);
}
