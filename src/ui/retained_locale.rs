//! Retained UI nodes whose text comes from the string tables
//! ([`crate::locale`]): they show the current language's text and font, and
//! follow when the language or a table changes.

use super::retained::*;
use crate::locale::{Arg, Strings};
use crate::text::Font;

/// A node's string table key, its arguments, and the font it had before.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextKey {
    key: String,
    args: Vec<(String, Arg)>,
    /// The node's font when the key was set: used by a language with no font.
    own_font: Font,
}

/// Which tables the keyed nodes show.
#[derive(Debug, Default)]
pub(crate) struct UiLocaleState {
    /// The [`Strings::revision`] last applied; 0: none yet.
    revision: u64,
    /// A key was set since then.
    dirty: bool,
}

impl Node {
    /// Sets the text as `set_text` does, keeping the key.
    pub(super) fn put_text(&mut self, text: &str) {
        self.text.clear();
        if self.widget == UiWidget::TextInput {
            self.text.extend(text.chars().filter(|c| !c.is_control()));
        } else {
            self.text.push_str(text);
        }
        self.caret = self.text.len();
        self.anchor = self.caret;
    }
}

impl Ui {
    /// Makes the node's text the string table key `key`, with `args` for its
    /// placeholders (`count` picks a plural form; see [`Strings::format`]).
    /// The engine fills in the current language's text before the next
    /// frame's layout, and again whenever the language or a table changes;
    /// until then it shows the key. The node is drawn with the language's
    /// font if it has one ([`Strings::font`]), else with the font it has now.
    /// [`Ui::set_text`] replaces the key with plain text.
    ///
    /// ```
    /// use ponko::locale::{Arg, StringTable, Strings};
    /// use ponko::ui::{Ui, UiLayout, UiWidget};
    ///
    /// let mut strings = Strings::default();
    /// strings.insert(StringTable::from_json(r#"{"language": "en", "strings": {"lives": "Lives: {count}"}}"#).unwrap());
    /// strings.insert(StringTable::from_json(r#"{"language": "it", "strings": {"lives": "Vite: {count}"}}"#).unwrap());
    ///
    /// let mut ui = Ui::default();
    /// let label = ui.add(UiWidget::Label, UiLayout::auto());
    /// ui.set_text_key(label, "lives", &[("count", Arg::from(3))]);
    /// ui.localize(&strings); // the engine does this every frame
    /// assert_eq!(ui.text(label), Some("Lives: 3"));
    ///
    /// strings.set_language("it");
    /// ui.localize(&strings);
    /// assert_eq!(ui.text(label), Some("Vite: 3"));
    /// ```
    pub fn set_text_key(&mut self, id: UiId, key: &str, args: &[(&str, Arg)]) {
        let Some(n) = self.node_mut(id) else { return };
        let own_font = match &n.key {
            Some(old) => old.own_font,
            None => n.paint.font,
        };
        if n.key.is_none() {
            n.put_text(key);
        }
        n.key = Some(Box::new(TextKey {
            key: key.to_string(),
            args: args.iter().map(|(name, arg)| (name.to_string(), arg.clone())).collect(),
            own_font,
        }));
        self.locale.dirty = true;
    }

    /// The string table key the node's text comes from, if it has one.
    ///
    /// ```
    /// use ponko::ui::{Ui, UiLayout, UiWidget};
    /// let mut ui = Ui::default();
    /// let label = ui.add(UiWidget::Label, UiLayout::auto());
    /// ui.set_text_key(label, "menu.play", &[]);
    /// assert_eq!(ui.text_key(label), Some("menu.play"));
    /// ui.set_text(label, "Play");
    /// assert_eq!(ui.text_key(label), None);
    /// ```
    #[must_use]
    pub fn text_key(&self, id: UiId) -> Option<&str> {
        Some(self.node(id)?.key.as_ref()?.key.as_str())
    }

    /// Gives every node that holds a key ([`Ui::set_text_key`]) its text and
    /// font in `strings`' current language. Does nothing when neither the
    /// tables nor the keys changed since the last call, so it is cheap to
    /// call every frame. The engine calls it for `ctx.ui` before each frame
    /// when the world has a [`Strings`] resource; a host that drives a `Ui`
    /// itself (a test, a [`UiPanel`](crate::d3::UiPanel)'s tree) calls it.
    pub fn localize(&mut self, strings: &Strings) {
        if !self.locale.dirty && self.locale.revision == strings.revision() {
            return;
        }
        self.locale.dirty = false;
        self.locale.revision = strings.revision();
        let font = strings.font();
        let mut text = String::new();
        for n in self.nodes.iter_mut().flatten() {
            let Some(key) = n.key.take() else { continue };
            let args: Vec<(&str, Arg)> = key
                .args
                .iter()
                .map(|(name, arg)| (name.as_str(), arg.clone()))
                .collect();
            strings.format_into(&key.key, &args, &mut text);
            drop(args);
            n.paint.font = font.unwrap_or(key.own_font);
            if n.text != text {
                n.put_text(&text);
            }
            n.key = Some(key);
        }
    }
}
