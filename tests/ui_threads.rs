//! `Ui` is `Send` and `Sync` by an `unsafe impl` (src/ui/retained.rs): its
//! layout engine's lengths could hold a raw pointer, which the engine never
//! makes. These tests move a tree to another thread and read one from
//! several at once, as a world-space panel (a `Ui` in a component) is in
//! parallel queries. They run under Miri (CONTRIBUTING.md, "Unsafe code"),
//! which skips the layout passes: measuring a leaf loads the built-in font,
//! which takes minutes there and has no unsafe code. The layouts themselves,
//! the values that could hold a pointer, are still read and changed.

use ponko::prelude::*;
use ponko::ui::{Ui, UiLayout};

const VIEW: Vec2 = Vec2::new(320.0, 240.0);
const LAY_OUT: bool = !cfg!(miri);

fn menu() -> (Ui, [ponko::ui::UiId; 3]) {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(4.0)).id();
    let title = ui.panel(root).layout(UiLayout::row(24.0)).id();
    let play = ui.panel(root).layout(UiLayout::row(30.0)).id();
    ui.push_modeless(root);
    if LAY_OUT {
        ui.layout(VIEW);
    }
    (ui, [root, title, play])
}

#[test]
fn a_ui_moved_to_another_thread_keeps_its_tree_and_lays_out_there() {
    let (ui, [root, title, play]) = menu();
    let before = ui.rect(play);
    let (ui, after) = std::thread::spawn(move || {
        let mut ui = ui;
        ui.set_layout(title, UiLayout::row(40.0));
        if LAY_OUT {
            ui.layout(VIEW * 2.0);
        }
        let after = ui.rect(play);
        (ui, after)
    })
    .join()
    .expect("the other thread finished");
    assert_eq!(
        ui.node_layout(title),
        Some(UiLayout::row(40.0)),
        "the change made there is kept"
    );
    assert_eq!(ui.parent(play), Some(root), "the tree is whole");
    if LAY_OUT {
        let (before, after) = (before.expect("laid out"), after.expect("still there"));
        assert_eq!(ui.rect(title).map(|r| r.max.y - r.min.y), Some(40.0));
        assert_eq!(
            after.min.y,
            before.min.y + 16.0,
            "the taller first row pushes the second down"
        );
        assert!(after.max.x > before.max.x, "the rows fill the wider view");
    }
}

#[test]
fn a_ui_is_read_from_several_threads_at_once() {
    let (ui, ids) = menu();
    // A layout is not `Send` (it could hold a pointer): threads return its text.
    let read = |ui: &Ui| ids.map(|id| (ui.node_layout(id).map(|l| format!("{l:?}")), ui.parent(id), ui.rect(id)));
    let shared = &ui;
    let seen: Vec<_> = std::thread::scope(|s| {
        let readers: Vec<_> = (0..3).map(|_| s.spawn(move || read(shared))).collect();
        readers
            .into_iter()
            .map(|r| r.join().expect("reader finished"))
            .collect()
    });
    let here = read(&ui);
    assert!(
        here.iter().all(|(layout, _, _)| layout.is_some()),
        "every node has a layout"
    );
    for seen in seen {
        assert_eq!(seen, here, "every thread reads the same tree");
    }
}
