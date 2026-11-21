//! Files the editor's authoring panels edit in place: a Tiled map, a
//! skeleton's animation graph, a glTF file's materials, a shader, a
//! terrain's heightmap. Each is a *document* kept here as JSON, by its asset
//! path: every edit is a step in the editor's one undo history
//! ([`Editor::set_doc`]), the panel that owns it applies the current value to
//! the world (it looks at [`Docs::version`]), and saving writes it back in
//! the file's own format with the writer the panel gave.

use std::collections::HashMap;

use serde_json::Value;

use super::{Editor, Merge, Undo};
use crate::error::Error;

/// Writes a document's value to its file (`key`, a path in `assets/`).
pub(crate) type Writer = fn(&str, &Value) -> Result<(), Error>;

/// One document: its value now, the value its file has, and how to write it.
struct Doc {
    value: Value,
    saved: Value,
    writer: Writer,
}

/// The open documents, by asset path.
#[derive(Default)]
pub(crate) struct Docs {
    docs: HashMap<String, Doc>,
    /// Goes up with every change to any document (an edit, undo, redo, an
    /// open): a panel applies its document again when it moved.
    version: u64,
}

impl Docs {
    /// Starts editing `key`, whose file holds `value`. A document already
    /// open keeps its edits.
    pub(crate) fn open(&mut self, key: &str, value: Value, writer: Writer) {
        if !self.docs.contains_key(key) {
            self.docs.insert(
                key.to_string(),
                Doc {
                    saved: value.clone(),
                    value,
                    writer,
                },
            );
            self.version += 1;
        }
    }

    /// Stops editing `key`, dropping what wasn't saved (its steps in the
    /// history then change nothing).
    pub(crate) fn close(&mut self, key: &str) {
        if self.docs.remove(key).is_some() {
            self.version += 1;
        }
    }

    /// The value of `key` now.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.docs.get(key).map(|d| &d.value)
    }

    /// See the field.
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    /// Whether `key` differs from its file.
    pub(crate) fn unsaved(&self, key: &str) -> bool {
        self.docs.get(key).is_some_and(|d| d.value != d.saved)
    }

    /// Whether any document differs from its file.
    pub(crate) fn any_unsaved(&self) -> bool {
        self.docs.values().any(|d| d.value != d.saved)
    }

    /// The documents that differ from their files, sorted.
    pub(crate) fn unsaved_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| d.value != d.saved)
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();
        keys
    }

    /// Writes `key` to its file.
    pub(crate) fn save(&mut self, key: &str) -> Result<(), Error> {
        let Some(doc) = self.docs.get_mut(key) else {
            return Ok(());
        };
        (doc.writer)(key, &doc.value)?;
        doc.saved = doc.value.clone();
        Ok(())
    }

    /// Writes every document that differs from its file; returns their paths.
    pub(crate) fn save_all(&mut self) -> Result<Vec<String>, Error> {
        let keys = self.unsaved_keys();
        for key in &keys {
            self.save(key)?;
        }
        Ok(keys)
    }

    /// Sets `key` to `value` and returns what it was (`Null` if it isn't
    /// open: a step of a closed document changes nothing).
    pub(super) fn swap(&mut self, key: &str, value: Value) -> Value {
        let Some(doc) = self.docs.get_mut(key) else {
            return Value::Null;
        };
        self.version += 1;
        std::mem::replace(&mut doc.value, value)
    }
}

impl Editor {
    /// Sets open document `key` to `value`, with undo. Nothing happens if it
    /// isn't open or already has that value; returns whether it changed.
    pub(crate) fn set_doc(&mut self, key: &str, value: Value) -> bool {
        if self.docs.get(key).is_none_or(|v| *v == value) {
            return false;
        }
        let before = self.docs.swap(key, value);
        self.record(Undo::Asset {
            key: key.to_string(),
            before,
        });
        true
    }

    /// [`set_doc`](Self::set_doc) for a change that goes on (a brush stroke,
    /// a dragged number): changes to one document, one after another, are
    /// one undo step until [`end_merge`](Self::end_merge) (the mouse button
    /// going up) or another change.
    pub(crate) fn set_doc_merged(&mut self, key: &str, value: Value) {
        let merge = Merge::Asset(key.to_string());
        if !self.merges(&merge) {
            if self.set_doc(key, value) {
                self.open = Some(merge);
            }
            return;
        }
        if self.docs.get(key).is_none_or(|v| *v == value) {
            return;
        }
        self.docs.swap(key, value);
        self.state = self.new_state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn never(_: &str, _: &Value) -> Result<(), Error> {
        Ok(())
    }

    #[test]
    fn document_edits_undo_and_redo_in_the_one_history() {
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, glam::Vec2::new(64.0, 64.0));
        let mut ed = Editor::default();
        ed.docs.open("a.tmj", json!({"n": 1}), never);
        ed.set_doc("a.tmj", json!({"n": 2}));
        assert!(ed.dirty() && ed.docs.unsaved("a.tmj"));
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ed.docs.get("a.tmj"), Some(&json!({"n": 1})));
        assert!(!ed.dirty() && !ed.docs.unsaved("a.tmj"));
        ed.redo(&mut ctx).unwrap();
        assert_eq!(ed.docs.get("a.tmj"), Some(&json!({"n": 2})));
    }

    #[test]
    fn a_stroke_of_merged_changes_is_one_undo_step() {
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, glam::Vec2::new(64.0, 64.0));
        let mut ed = Editor::default();
        ed.docs.open("h.png", json!([0, 0]), never);
        for i in 1..5 {
            ed.set_doc_merged("h.png", json!([i, 0]));
        }
        ed.end_merge();
        ed.set_doc_merged("h.png", json!([9, 9]));
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ed.docs.get("h.png"), Some(&json!([4, 0])));
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ed.docs.get("h.png"), Some(&json!([0, 0])));
        assert!(!ed.can_undo());
    }

    #[test]
    fn saving_writes_only_changed_documents_and_marks_them_saved() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static WRITES: AtomicUsize = AtomicUsize::new(0);
        fn count(_: &str, _: &Value) -> Result<(), Error> {
            WRITES.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        let mut ed = Editor::default();
        ed.docs.open("a", json!(1), count);
        ed.docs.open("b", json!(1), count);
        ed.set_doc("a", json!(2));
        assert_eq!(ed.docs.save_all().unwrap(), vec!["a".to_string()]);
        assert_eq!(WRITES.load(Ordering::SeqCst), 1);
        assert!(ed.docs.unsaved_keys().is_empty());
    }
}
