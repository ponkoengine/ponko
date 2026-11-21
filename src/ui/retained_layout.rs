//! Skipping layout when nothing changed. Taffy keeps a cache per node and
//! marks a node dirty (with its ancestors) when its style or children
//! change; the tree only has to mark the nodes whose *text* measures
//! differently, which Taffy can't see: their text, font, size, wrapping,
//! widget or rich markup changed, or, for every node, the window size,
//! the scale or the fonts did. A root with nothing dirty keeps its layout.
use super::retained::{Node, Ui, UiWidget};
use crate::text::Font;
use glam::Vec2;

/// What a node's text was measured from in the last layout.
#[derive(Default)]
pub(super) struct Measured {
    /// `None` until its first layout.
    widget: Option<UiWidget>,
    text: String,
    font: Font,
    size: u32,
    wrap: bool,
    /// The parsed markup's revision, for a rich node.
    rich: Option<u64>,
}

/// What every node's layout was computed for: when it changes, every
/// node is measured again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LayoutKey {
    pub viewport: Vec2,
    pub safe_area: Option<crate::camera::Rect>,
    pub scale: f32,
    pub pixels: f32,
    pub external_fonts: bool,
    pub fonts: u64,
}

/// The layouts so far ([`Ui::layout_passes`]) and what the last one was
/// computed for.
#[derive(Default)]
pub(super) struct LayoutCache {
    pub key: Option<LayoutKey>,
    pub passes: u64,
}

impl Node {
    /// Whether what its text is measured from changed since the last
    /// layout; remembers it. Copies the text only when it changed.
    pub(super) fn measure_changed(&mut self) -> bool {
        let rich = self.rich.as_deref().map(|r| r.parsed);
        let m = &mut self.measured;
        if m.widget == Some(self.widget)
            && m.font == self.paint.font
            && m.size == self.paint.font_size.to_bits()
            && m.wrap == self.paint.wrap
            && m.rich == rich
            && m.text == self.text
        {
            return false;
        }
        m.widget = Some(self.widget);
        m.font = self.paint.font;
        m.size = self.paint.font_size.to_bits();
        m.wrap = self.paint.wrap;
        m.rich = rich;
        m.text.clear();
        m.text.push_str(&self.text);
        true
    }
}

impl Ui {
    /// How many times the tree has been laid out: Taffy run over the
    /// screens and HUDs, their text measured. The engine asks for a layout
    /// before the frame's systems and again before drawing, but one runs
    /// only when something changed since the last: the window size, the
    /// scale, the fonts, a node's layout, text, font or size, a node added,
    /// removed or shown. A frame where nothing changed costs no layout; one
    /// where a label's text changed measures that label again and lays out
    /// the nodes it moves. Scrolling and animations of drawn properties
    /// move what was laid out without a layout.
    ///
    /// For tests and profiling: the count says whether a UI that should be
    /// still is laid out every frame.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiInput};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let score = ui.label(root, "Score: 0").id();
    /// ui.show_hud(root);
    /// ui.frame(vec2(800.0, 600.0), &UiInput::default());
    /// let laid_out = ui.layout_passes();
    /// ui.set_text(score, "Score: 0"); // the same text
    /// ui.frame(vec2(800.0, 600.0), &UiInput::default());
    /// assert_eq!(ui.layout_passes(), laid_out, "nothing changed");
    /// ui.set_text(score, "Score: 10");
    /// ui.frame(vec2(800.0, 600.0), &UiInput::default());
    /// assert_eq!(ui.layout_passes(), laid_out + 1);
    /// ```
    pub fn layout_passes(&self) -> u64 {
        self.layout_cache.passes
    }

    /// Marks for Taffy the nodes whose text measures differently now, and
    /// every node when `key` differs from the last layout's. True if a
    /// shown root has something to lay out.
    pub(super) fn mark_layout_changes(&mut self, key: LayoutKey) -> bool {
        let all = self.layout_cache.key != Some(key);
        for n in self.nodes.iter_mut().flatten() {
            if n.measure_changed() | all {
                let _ = self.tree.mark_dirty(n.layout);
            }
        }
        self.layout_cache.key = Some(key);
        let roots = self.huds.iter().chain(self.screens.iter().map(|s| &s.root));
        let mut stale = false;
        for root in roots {
            if let Some(n) = self.node(*root) {
                stale |= self.tree.dirty(n.layout).unwrap_or(true);
            }
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::ui::{Ui, UiInput, UiLayout};

    const VIEW: Vec2 = Vec2::new(800.0, 600.0);

    /// A menu with a HUD; one frame laid out.
    fn menu() -> (Ui, crate::ui::UiId, crate::ui::UiId) {
        let mut ui = Ui::default();
        let hud = ui.root().layout(UiLayout::fill().padding(8.0)).id();
        let score = ui.label(hud, "Score: 0").id();
        ui.show_hud(hud);
        let menu = ui.root().layout(UiLayout::centered(300.0, 200.0).gap(8.0)).id();
        let play = ui.button(menu, "Play").layout(UiLayout::row(40.0)).id();
        ui.button(menu, "Quit").layout(UiLayout::row(40.0));
        ui.push_modeless(menu);
        ui.frame(VIEW, &UiInput::default());
        (ui, score, play)
    }

    fn passes_over(ui: &mut Ui, frames: usize, input: &UiInput) -> u64 {
        let before = ui.layout_passes();
        for _ in 0..frames {
            ui.frame(VIEW, input);
        }
        ui.layout_passes() - before
    }

    #[test]
    fn first_frame_lays_out_once() {
        let (ui, ..) = menu();
        assert_eq!(ui.layout_passes(), 1);
    }

    #[test]
    fn still_frames_cost_no_layout() {
        let (mut ui, score, _) = menu();
        assert_eq!(passes_over(&mut ui, 10, &UiInput::default()), 0);
        // The pointer resting on a button, frame after frame.
        let still = UiInput {
            pointer: vec2(400.0, 280.0),
            dt: 1.0 / 60.0,
            ..Default::default()
        };
        ui.frame(VIEW, &still);
        assert_eq!(passes_over(&mut ui, 10, &still), 0);
        // A HUD system writing the text the label already shows.
        let before = ui.layout_passes();
        for _ in 0..10 {
            ui.set_text(score, "Score: 0");
            ui.frame(VIEW, &still);
        }
        assert_eq!(ui.layout_passes(), before);
    }

    #[test]
    fn changed_text_is_laid_out_again() {
        let (mut ui, _, _) = menu();
        // In a row a label is as wide as its text.
        let bar = ui.root().layout(UiLayout::row(30.0)).id();
        let score = ui.label(bar, "Score: 0").id();
        ui.show_hud(bar);
        ui.frame(VIEW, &UiInput::default());
        let narrow = ui.rect(score).unwrap();
        ui.set_text(score, "Score: 1,000,000");
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        assert!(ui.rect(score).unwrap().size().x > narrow.size().x, "measured again");
        assert_eq!(passes_over(&mut ui, 5, &UiInput::default()), 0);
    }

    #[test]
    fn changed_paint_size_is_laid_out_again() {
        let (mut ui, score, _) = menu();
        let small = ui.rect(score).unwrap();
        let mut paint = ui.paint(score).unwrap();
        paint.font_size *= 2.0;
        ui.set_paint(score, paint);
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        assert!(ui.rect(score).unwrap().size().y > small.size().y);
    }

    #[test]
    fn changed_layout_is_laid_out_again() {
        let (mut ui, _, play) = menu();
        ui.set_layout(play, UiLayout::row(60.0));
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        assert_eq!(ui.rect(play).unwrap().size().y, 60.0);
        // The same layout again changes nothing.
        ui.set_layout(play, UiLayout::row(60.0));
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 0);
    }

    #[test]
    fn window_size_and_scale_lay_out_again() {
        let (mut ui, _, play) = menu();
        let before = ui.layout_passes();
        ui.frame(vec2(1024.0, 768.0), &UiInput::default());
        assert_eq!(ui.layout_passes(), before + 1);
        let at = ui.rect(play).unwrap();
        ui.set_scale(2.0);
        ui.frame(vec2(1024.0, 768.0), &UiInput::default());
        assert_eq!(ui.layout_passes(), before + 2);
        assert_eq!(ui.rect(play).unwrap().size().x, at.size().x * 2.0);
    }

    #[test]
    fn added_removed_and_pushed_nodes_lay_out_again() {
        let (mut ui, score, _) = menu();
        let hud = ui.parent(score).unwrap();
        let coins = ui.label(hud, "Coins: 3").id();
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        assert!(ui.rect(coins).unwrap().size().y > 0.0);
        ui.remove(coins);
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        // A screen built while another was up, pushed later.
        let dialog = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
        let ok = ui.button(dialog, "OK").layout(UiLayout::fill()).id();
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 0, "not shown yet");
        ui.push_modal(dialog);
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 1);
        assert_eq!(ui.rect(ok).unwrap().size(), vec2(200.0, 100.0));
        // Popped and pushed again: its layout still holds.
        ui.pop_screen();
        ui.push_modal(dialog);
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 0);
        assert_eq!(ui.rect(ok).unwrap().size(), vec2(200.0, 100.0));
    }

    #[test]
    fn typing_lays_out_the_frame_it_types() {
        let mut ui = Ui::default();
        let root = ui.root().layout(UiLayout::fill()).id();
        let name = ui.text_input(root).layout(UiLayout::auto()).focused().id();
        ui.push_modeless(root);
        ui.frame(VIEW, &UiInput::default());
        let typed = UiInput {
            text: "Ada".into(),
            ..Default::default()
        };
        assert_eq!(passes_over(&mut ui, 1, &typed), 1);
        assert_eq!(ui.text(name), Some("Ada"));
        assert_eq!(passes_over(&mut ui, 3, &UiInput::default()), 0);
    }

    #[test]
    fn scrolling_moves_nodes_without_a_layout() {
        let mut ui = Ui::default();
        let root = ui.root().layout(UiLayout::fill()).id();
        let scroll = ui.scroll_area(root).layout(UiLayout::fixed(200.0, 100.0)).id();
        let first = ui.label(scroll, "First").layout(UiLayout::row(40.0)).id();
        for i in 0..10 {
            ui.label(scroll, format!("Row {i}")).layout(UiLayout::row(40.0));
        }
        ui.push_modeless(root);
        ui.frame(VIEW, &UiInput::default());
        let top = ui.rect(first).unwrap().min.y;
        ui.set_scroll(scroll, vec2(0.0, 30.0));
        assert_eq!(passes_over(&mut ui, 1, &UiInput::default()), 0);
        assert_eq!(ui.rect(first).unwrap().min.y, top - 30.0, "placed where it scrolled");
    }
}
