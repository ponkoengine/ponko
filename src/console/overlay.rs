//! The retained developer console. UI owns the prompt's text editing,
//! navigation focus, IME and scrolling. This state owns past commands and
//! output, and only listens to console shortcuts while its screen is on top.

use std::collections::VecDeque;

use glam::Vec2;

use super::{MAX_LINE_BYTES, Registry};
use crate::color::Color;
use crate::input::{Input, Key};
use crate::ui::{Ui, UiEvent, UiId, UiInput, UiLayout};

const HISTORY: usize = 64;
const SCROLLBACK: usize = 256;
const MAX_LOG_LINE_BYTES: usize = 2048;

#[derive(Clone, Copy)]
struct View {
    root: UiId,
    input: UiId,
    output: UiId,
    scroll: UiId,
}

impl View {
    fn new(ui: &mut Ui) -> Self {
        let root = ui
            .root()
            .layout(UiLayout::anchored(0.0, 0.0, 1.0, 0.55).padding(8.0).gap(6.0))
            .background(Color::rgb(0.06, 0.08, 0.11))
            .name("ponko.console")
            .id();
        ui.label(
            root,
            "Developer Console · help · Tab completes · ↑/↓ history · Esc closes",
        )
        .layout(UiLayout::row(22.0))
        .color(Color::rgb(0.65, 0.78, 0.88))
        .size(14.0);
        let scroll = ui
            .scroll_area(root)
            .layout(UiLayout::row(0.0).vertical().grow(1.0))
            .name("ponko.console.scroll")
            .id();
        let output = ui
            .label(scroll, "")
            .color(Color::WHITE)
            .size(15.0)
            .wrap()
            .name("ponko.console.output")
            .id();
        let input = ui
            .text_input(root)
            .layout(UiLayout::row(30.0))
            .name("ponko.console.input")
            .focused()
            .id();
        Self {
            root,
            input,
            output,
            scroll,
        }
    }
}

pub(crate) struct Overlay {
    pub(crate) key: Key,
    view: Option<View>,
    history: VecDeque<String>,
    history_at: Option<usize>,
    draft: String,
    lines: VecDeque<String>,
    complete: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            key: Key::Backquote,
            view: None,
            history: VecDeque::new(),
            history_at: None,
            draft: String::new(),
            lines: VecDeque::new(),
            complete: false,
        }
    }
}

impl Overlay {
    pub(crate) fn prepare(&mut self, registry: &Registry, ui: &mut Ui, keys: &Input, input: &mut UiInput) {
        if !registry.enabled() {
            return;
        }
        self.complete = false;
        if keys.key_pressed(self.key) {
            let view = match self.view.filter(|view| ui.node_layout(view.root).is_some()) {
                Some(view) => view,
                None => {
                    let view = View::new(ui);
                    self.view = Some(view);
                    self.refresh(ui);
                    view
                }
            };
            if ui.top_screen() == Some(view.root) {
                input.text.clear();
                // Use the UI's cancellation path to restore previous focus
                // and consume this frame's input when returning to the game.
                input.cancel = true;
                input.composing = false;
                input.preedit.clear();
                return;
            }
            if !ui.is_screen(view.root) {
                input.text.clear();
                ui.push_modal(view.root);
                self.history_at = None;
                input.confirm = false;
                input.cancel = false;
            }
        }
        let Some(view) = self.view.filter(|view| ui.top_screen() == Some(view.root)) else {
            return;
        };
        if input.composing || !input.preedit.is_empty() {
            return;
        }
        if input.next || input.previous {
            input.next = false;
            input.previous = false;
            self.complete = ui.set_focus(view.input);
        }
        if ui.focus() == Some(view.input) {
            let up = keys.key_pressed(Key::Up);
            let down = keys.key_pressed(Key::Down);
            if up || down {
                self.recall(ui, view.input, up);
                input.direction.y = 0.0;
            }
        }
        let pages = i8::from(keys.key_pressed(Key::PageDown)) - i8::from(keys.key_pressed(Key::PageUp));
        if pages != 0
            && let (Some(at), Some(rect)) = (ui.scroll(view.scroll), ui.rect(view.scroll))
        {
            ui.set_scroll(view.scroll, at + Vec2::Y * rect.size().y * f32::from(pages));
        }
        // Bound prompt growth on keyboard, paste and IME input.
        let remaining = MAX_LINE_BYTES.saturating_sub(ui.text(view.input).unwrap_or_default().len());
        input
            .text
            .truncate(input.text.floor_char_boundary(remaining.min(input.text.len())));
    }

    fn recall(&mut self, ui: &mut Ui, input: UiId, up: bool) {
        let len = self.history.len();
        let previous = self.history_at;
        let current = previous.unwrap_or(len);
        let next = if up {
            current.saturating_sub(1)
        } else {
            (current + 1).min(len)
        };
        if next == current {
            return;
        }
        if previous.is_none() {
            self.draft = ui.text(input).unwrap_or_default().to_owned();
        }
        if let Some(line) = self.history.get(next) {
            self.history_at = Some(next);
            ui.set_text(input, line);
        } else {
            self.history_at = None;
            ui.set_text(input, &self.draft);
        }
    }

    pub(crate) fn finish(&mut self, registry: &Registry, ui: &mut Ui, submitted: bool) -> Option<String> {
        let view = self.view.filter(|view| ui.top_screen() == Some(view.root))?;
        // Read after the retained UI applies text typed in this pass.
        if submitted {
            let line = ui.text(view.input).unwrap_or_default().trim().to_owned();
            ui.set_text(view.input, "");
            self.history_at = None;
            self.draft.clear();
            if line.is_empty() {
                return None;
            }
            if self.history.back() != Some(&line) {
                if self.history.len() == HISTORY {
                    self.history.pop_front();
                }
                self.history.push_back(line.clone());
            }
            self.append(&format!("> {line}"));
            return Some(line);
        }
        if self.complete {
            self.complete = false;
            self.complete_input(registry, ui, view.input);
        }
        None
    }

    fn complete_input(&mut self, registry: &Registry, ui: &mut Ui, input: UiId) {
        let line = ui.text(input).unwrap_or_default().trim_start();
        let (head, prefix) = if let Some(rest) = line.strip_prefix("help ") {
            ("help ", rest.trim_start())
        } else {
            ("", line)
        };
        if prefix.contains(char::is_whitespace) {
            return;
        }
        let mut matches = registry.names().filter(|name| name.starts_with(prefix));
        let Some(first) = matches.next() else { return };
        let others: Vec<_> = matches.collect();
        if others.is_empty() {
            ui.set_text(
                input,
                format!("{head}{first}{}", if head.is_empty() { " " } else { "" }),
            );
        } else {
            // Entry names are ASCII. Extend through the prefix they share and
            // put the list in output so the user can choose among them.
            let common = others.iter().fold(first.len(), |len, name| {
                first
                    .bytes()
                    .take(len)
                    .zip(name.bytes())
                    .take_while(|(a, b)| a == b)
                    .count()
            });
            ui.set_text(input, format!("{head}{}", &first[..common]));
            for name in std::iter::once(first).chain(others) {
                self.append(name);
            }
            self.refresh(ui);
        }
    }

    pub(crate) fn event(&self, event: UiEvent) -> bool {
        self.view.is_some_and(|view| event == UiEvent::Submitted(view.input))
    }

    pub(crate) fn reply(&mut self, ui: &mut Ui, result: Result<String, crate::error::Error>) {
        match result {
            Ok(output) => self.append(&output),
            Err(error) => self.append(&format!("Error: {}", error.message())),
        }
        self.refresh(ui);
    }

    fn append(&mut self, text: &str) {
        for line in text.lines() {
            if self.lines.len() == SCROLLBACK {
                self.lines.pop_front();
            }
            let max = line.floor_char_boundary(MAX_LOG_LINE_BYTES.min(line.len()));
            let mut kept = line[..max].to_owned();
            if max < line.len() {
                kept.push('…');
            }
            self.lines.push_back(kept);
        }
    }

    fn refresh(&self, ui: &mut Ui) {
        if let Some(view) = self.view {
            let text = self.lines.iter().map(String::as_str).collect::<Vec<_>>().join("\n");
            ui.set_text(view.output, text);
            // The next layout clamps the scroll offset to the new content.
            ui.set_scroll(view.scroll, Vec2::new(0.0, f32::MAX));
        }
    }
}
