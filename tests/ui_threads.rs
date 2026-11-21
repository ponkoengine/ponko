//! `Ui` is `Send` and `Sync` by an `unsafe impl` (src/ui/retained.rs): its
//! layout engine's lengths could hold a raw pointer, which the engine never
//! makes. These tests move a laid-out tree to another thread and read one
//! from several at once, as a world-space panel (a `Ui` in a component) is in
//! parallel queries. They run under Miri (CONTRIBUTING.md, "Unsafe code").

use ponko::prelude::*;
use ponko::ui::{Ui, UiLayout};

const VIEW: Vec2 = Vec2::new(320.0, 240.0);

fn menu() -> (Ui, [ponko::ui::UiId; 3]) {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(4.0)).id();
    // Panels, not text: shaping is slow under Miri and has no unsafe code.
    let title = ui.panel(root).layout(UiLayout::row(24.0)).id();
    let play = ui.panel(root).layout(UiLayout::row(30.0)).id();
    ui.push_modeless(root);
    ui.layout(VIEW);
    (ui, [root, title, play])
}

#[test]
fn a_ui_moved_to_another_thread_keeps_its_tree_and_lays_out_there() {
    let (ui, [_, title, play]) = menu();
    let before = ui.rect(play).expect("laid out");
    let (ui, after) = std::thread::spawn(move || {
        let mut ui = ui;
        ui.set_layout(title, UiLayout::row(40.0));
        ui.layout(VIEW * 2.0);
        let after = ui.rect(play).expect("still there");
        (ui, after)
    })
    .join()
    .expect("the other thread finished");
    assert_eq!(ui.rect(title).map(|r| r.max.y - r.min.y), Some(40.0), "the change made there is kept");
    assert_eq!(after.min.y, before.min.y + 16.0, "the taller first row pushes the second down");
    assert!(after.max.x > before.max.x, "the rows fill the wider view");
}

#[test]
fn a_ui_is_read_from_several_threads_at_once() {
    let (ui, [root, title, play]) = menu();
    let shared = &ui;
    let rects: Vec<_> = std::thread::scope(|s| {
        let readers: Vec<_> = (0..3)
            .map(|_| s.spawn(move || [root, title, play].map(|id| shared.rect(id))))
            .collect();
        readers.into_iter().map(|r| r.join().expect("reader finished")).collect()
    });
    let here = [root, title, play].map(|id| ui.rect(id));
    for seen in rects {
        assert_eq!(seen, here, "every thread reads the same layout");
    }
}
