//! Clipboard shortcuts for the focused retained text field.
#![deny(missing_docs)]
use crate::ui::{Ui, UiEvent, UiWidget};
impl Ui {
    /// Selected UTF-8 text, only when a text input has focus and is not composing.
    pub fn selected_text(&self) -> Option<&str> {
        let n = self.node(self.focus()?)?;
        if n.widget != UiWidget::TextInput || !n.preedit.is_empty() {
            return None;
        }
        Some(&n.text[n.caret.min(n.anchor)..n.caret.max(n.anchor)])
    }
    /// Delete a focused field's selection after a successful clipboard copy.
    pub fn cut_selection(&mut self) {
        let Some(id) = self.focus() else {
            return;
        };
        if self.selected_text().is_none_or(str::is_empty) {
            return;
        }
        let n = self.node_mut(id).unwrap();
        let start = n.caret.min(n.anchor);
        n.text.replace_range(start..n.caret.max(n.anchor), "");
        n.caret = start;
        n.anchor = start;
        self.events.push_back(UiEvent::Changed(id));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        actions::InputEvent,
        ctx::Ctx,
        input::Key,
        ui::{UiInput, UiLayout},
        window::*,
    };
    use glam::vec2;
    use std::{cell::RefCell, rc::Rc};
    struct Fake(Rc<RefCell<String>>, bool);
    impl WindowBackend for Fake {
        fn request(&mut self, _: &WindowRequest) -> Result<(), crate::error::Error> {
            Ok(())
        }
        fn state(&self) -> WindowState {
            WindowState::default()
        }
        fn monitors(&self) -> Vec<Monitor> {
            vec![]
        }
        fn clipboard_write(&mut self, s: &str) -> Result<(), crate::error::Error> {
            *self.0.borrow_mut() = s.into();
            Ok(())
        }
        fn clipboard_read(&mut self) -> Result<Option<String>, crate::error::Error> {
            if self.1 {
                self.1 = false;
                Ok(None)
            } else {
                Ok(Some(self.0.borrow().clone()))
            }
        }
    }
    #[test]
    fn screen_shortcuts_copy_cut_and_complete_async_paste_without_inserting_shortcut_letters() {
        let mut c = Ctx::new(1.0 / 60.0, vec2(800.0, 600.0));
        let clipboard = Rc::new(RefCell::new(String::new()));
        c.window = WindowService::with_backend(Fake(clipboard.clone(), true));
        let root = c.ui.root().id();
        let field =
            c.ui.text_input(root)
                .text("世界")
                .layout(UiLayout::fixed(200.0, 30.0))
                .id();
        c.ui.push_modeless(root);
        assert!(c.ui.set_focus(field));
        c.ui.frame(
            c.window_size(),
            &UiInput {
                select_all: true,
                ..Default::default()
            },
        );
        c.input.feed(InputEvent::Key(Key::LeftCtrl, true));
        c.input.feed(InputEvent::Key(Key::C, true));
        c.input.insert_text("c");
        c.update_retained_ui();
        assert_eq!(&*clipboard.borrow(), "世界");
        assert_eq!(c.ui.text(field), Some("世界"));
        c.input.end_frame();
        c.input.feed(InputEvent::Key(Key::C, false));
        c.input.feed(InputEvent::Key(Key::X, true));
        c.update_retained_ui();
        assert_eq!(c.ui.text(field), Some(""));
        c.input.end_frame();
        c.input.feed(InputEvent::Key(Key::X, false));
        c.input.feed(InputEvent::Key(Key::V, true));
        c.update_retained_ui();
        assert_eq!(c.ui.text(field), Some("")); // browser promise pending
        c.input.end_frame();
        c.update_retained_ui();
        assert_eq!(c.ui.text(field), Some("世界"));
    }
}
