//! The retained UI's virtual list: one frame of a list scrolling three rows,
//! for lists of 100, 10,000 and 1,000,000 items. The list keeps nodes only
//! for the rows on screen and reuses them, so the three cost the same.
//!
//! - `ui/list-scroll-100`, `ui/list-scroll-10k`, `ui/list-scroll-1m`: a
//!   300 x 480 list of 24-unit rows (21 row nodes), scrolled by the wheel
//!   each frame, wrapping at the end; the frame lays out, moves the rows and
//!   gives them their items' text.
//! - `ui/list-still-10k`: the 10,000 list's frame without scrolling.
use ponko::prelude::{Vec2, vec2};
use ponko::ui::{Ui, UiId, UiInput, UiLayout};

use crate::Bench;

const VIEW: Vec2 = Vec2::new(640.0, 520.0);
const ROW: f32 = 24.0;

struct Scrolling {
    ui: Ui,
    list: UiId,
    len: usize,
    first: usize,
}

fn list(len: usize) -> Scrolling {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().padding(20.0)).id();
    let list = ui.list(root, ROW).layout(UiLayout::fixed(300.0, 480.0)).id();
    ui.set_items(list, (0..len).map(|i| format!("Save {i}")));
    ui.push_modeless(root);
    ui.frame(VIEW, &UiInput::default());
    Scrolling {
        ui,
        list,
        len,
        first: 0,
    }
}

pub fn run(b: &mut Bench) {
    // The same list standing still: what a frame costs without the rows moving.
    b.steady(
        "ui/list-still-10k",
        300,
        || list(10_000),
        |s| {
            s.ui.frame(VIEW, &UiInput::default());
            s.ui.events().for_each(drop);
        },
    );
    for (name, len) in [
        ("ui/list-scroll-100", 100),
        ("ui/list-scroll-10k", 10_000),
        ("ui/list-scroll-1m", 1_000_000),
    ] {
        b.steady(
            name,
            300,
            || list(len),
            |s| {
                // Three rows down, back to the top past the end.
                s.first = if s.first + 3 + 20 >= s.len { 0 } else { s.first + 3 };
                if s.first == 0 {
                    s.ui.scroll_to_item(s.list, 0);
                }
                let input = UiInput {
                    pointer: vec2(150.0, 200.0),
                    wheel: vec2(0.0, if s.first == 0 { 0.0 } else { 3.0 * ROW }),
                    ..Default::default()
                };
                s.ui.frame(VIEW, &input);
                s.ui.events().for_each(drop);
            },
        );
    }
}
