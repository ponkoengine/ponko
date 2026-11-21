//! Drives the editor's window in tests of the authoring panels, as a person
//! would: fake mouse and keyboard input through the editor's own frame
//! (no window), on a screen tall enough for the inspector's long sections.

use egui::Key;
use glam::Vec2;
use ponko_ecs::Entity;

use super::Panels;
use super::app::App;
use crate::ctx::Ctx;

/// A tall screen: the particle section and the authoring windows fit
/// without scrolling.
pub(crate) const SCREEN: egui::Vec2 = egui::vec2(1400.0, 1500.0);

/// The editor app in edit mode over a world the test fills.
pub(crate) struct Harness {
    pub egui: egui::Context,
    pub panels: Panels,
    pub ctx: Ctx,
    time: f64,
}

impl Harness {
    /// An empty world in edit mode, one frame run, with nothing remembered.
    pub(crate) fn new() -> Self {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SCREEN.x, SCREEN.y));
        // No GPU: textures a level or a palette loads are handles that
        // remember their size.
        ctx.set_headless(true);
        let panels = Panels {
            app: Some(App::with_settings(None, None, "test".into())),
            ..Default::default()
        };
        let egui = egui::Context::default();
        egui.all_styles_mut(|s| s.animation_time = 0.0);
        let mut h = Self {
            egui,
            panels,
            ctx,
            time: 0.0,
        };
        h.frame(vec![]);
        h
    }

    /// Selects `e` as a click in the hierarchy would.
    pub(crate) fn select(&mut self, e: Entity) {
        self.panels.select(Some(e), false);
    }

    /// One frame with these events; returns what it drew.
    pub(crate) fn frame(&mut self, mut events: Vec<egui::Event>) -> Vec<egui::epaint::ClippedShape> {
        let held = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .fold(egui::Modifiers::NONE, |a, b| a | b);
        events.insert(0, egui::Event::ModifiersChanged(held));
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        let mut out = self.panels.run(&self.egui, input, &mut self.ctx);
        out.textures_delta.clear();
        out.shapes
    }

    /// Moves to `at`, presses and lets go there.
    pub(crate) fn click(&mut self, at: Vec2) {
        self.frame(vec![moved(at)]);
        self.frame(vec![button(at, true)]);
        self.frame(vec![button(at, false)]);
        self.frame(vec![]);
    }

    /// Two clicks at `at`, quickly, after a pause long enough that an
    /// earlier click doesn't count towards it.
    pub(crate) fn double_click(&mut self, at: Vec2) {
        for _ in 0..30 {
            self.frame(vec![]);
        }
        self.frame(vec![moved(at)]);
        self.frame(vec![button(at, true)]);
        self.frame(vec![button(at, false)]);
        self.frame(vec![button(at, true)]);
        self.frame(vec![button(at, false)]);
        self.frame(vec![]);
    }

    /// Presses at `from`, moves on to `to` in `steps` frames, lets go there.
    pub(crate) fn drag(&mut self, from: Vec2, to: Vec2, steps: usize) {
        self.press(from);
        for i in 1..=steps {
            self.frame(vec![moved(from.lerp(to, i as f32 / steps as f32))]);
        }
        self.release(to);
    }

    /// Moves to `at` and presses the button there.
    pub(crate) fn press(&mut self, at: Vec2) {
        self.frame(vec![moved(at)]);
        self.frame(vec![button(at, true)]);
    }

    /// Lets the button go at `at`.
    pub(crate) fn release(&mut self, at: Vec2) {
        self.frame(vec![button(at, false)]);
        self.frame(vec![]);
    }

    /// Types `text` into the focused field and presses Enter.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.frame(vec![
            key(Key::A, egui::Modifiers::COMMAND),
            egui::Event::Text(text.into()),
        ]);
        self.frame(vec![key(Key::Enter, egui::Modifiers::NONE)]);
        self.frame(vec![]);
    }

    /// Ctrl+Z (⌘Z).
    pub(crate) fn undo(&mut self) {
        self.frame(vec![key(Key::Z, egui::Modifiers::COMMAND)]);
    }

    /// Ctrl+Shift+Z.
    pub(crate) fn redo(&mut self) {
        self.frame(vec![key(Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
    }

    /// Ctrl+S.
    pub(crate) fn save(&mut self) {
        self.frame(vec![key(Key::S, egui::Modifiers::COMMAND)]);
    }

    /// A rectangle a panel kept in egui's memory under `id` (where it drew
    /// a graph or canvas), in screen points.
    pub(crate) fn rect(&self, id: egui::Id) -> egui::Rect {
        self.egui
            .data(|d| d.get_temp::<egui::Rect>(id))
            .unwrap_or_else(|| panic!("nothing was drawn under {id:?}"))
    }

    /// The middle of the first text drawn that `wanted` accepts, after one
    /// more frame.
    pub(crate) fn find(&mut self, wanted: impl Fn(&str) -> bool) -> Option<Vec2> {
        let shapes = self.frame(vec![]);
        text_at(&shapes, wanted)
    }

    /// Clicks the last text `wanted` accepts: the one on top, such as an
    /// entry of a menu that just opened.
    pub(crate) fn click_last_text(&mut self, wanted: impl Fn(&str) -> bool) {
        let shapes = self.frame(vec![]);
        let at = texts(&shapes)
            .into_iter()
            .rev()
            .find(|(t, _)| wanted(t))
            .map(|(_, at)| at)
            .expect("the text is drawn");
        self.click(at);
    }

    /// Clicks the first text `wanted` accepts.
    pub(crate) fn click_text(&mut self, wanted: impl Fn(&str) -> bool) {
        let at = self.find(wanted).expect("the text is drawn");
        self.click(at);
    }
}

/// The middle of the first text drawn that `wanted` accepts.
pub(crate) fn text_at(shapes: &[egui::epaint::ClippedShape], wanted: impl Fn(&str) -> bool) -> Option<Vec2> {
    fn find(shape: &egui::Shape, wanted: &dyn Fn(&str) -> bool) -> Option<Vec2> {
        match shape {
            egui::Shape::Text(t) if wanted(t.galley.text()) => {
                let c = t.galley.rect.translate(t.pos.to_vec2()).center();
                Some(Vec2::new(c.x, c.y))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, wanted)),
            _ => None,
        }
    }
    shapes.iter().find_map(|c| find(&c.shape, &wanted))
}

/// Every text drawn, with its middle.
pub(crate) fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Vec2)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, Vec2)>) {
        match shape {
            egui::Shape::Text(t) => {
                let c = t.galley.rect.translate(t.pos.to_vec2()).center();
                out.push((t.galley.text().to_string(), Vec2::new(c.x, c.y)));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// The field on the row of text `label`: the nearest text to its right on
/// the same line (a number field shows its number).
pub(crate) fn field_right_of(shapes: &[egui::epaint::ClippedShape], label: &str) -> Vec2 {
    let at = text_at(shapes, |t| t == label).unwrap_or_else(|| panic!("{label:?} is drawn"));
    texts(shapes)
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| (p.y - at.y).abs() < 4.0 && p.x > at.x + 20.0)
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap_or_else(|| panic!("a field right of {label:?}"))
}

pub(crate) fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

pub(crate) fn button(at: Vec2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(at.x, at.y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

pub(crate) fn moved(at: Vec2) -> egui::Event {
    egui::Event::PointerMoved(egui::pos2(at.x, at.y))
}
