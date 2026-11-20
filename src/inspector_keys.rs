//! The editor's keyboard shortcuts: each action and the keys bound to it.
//! The panels ask the keymap ([`Keymap::pressed`]) instead of checking keys
//! themselves, so the editor app can list the shortcuts and let them be
//! rebound (saved with its settings). The defaults are the keys the
//! inspector has always used.
// Listing, rebinding and saving them is the editor app's.
#![cfg_attr(not(feature = "editor"), allow(dead_code))]

use egui::{Key, KeyboardShortcut, Modifiers};

/// Something a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Duplicate,
    Delete,
    Rename,
    Cancel,
    Move,
    Rotate,
    Scale,
    Frame,
    Play,
    Pause,
    Step,
    Stop,
    Search,
    Shortcuts,
}

impl Action {
    /// Every action, in the order the shortcuts list shows them.
    pub const ALL: [Action; 20] = [
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Undo,
        Action::Redo,
        Action::Duplicate,
        Action::Delete,
        Action::Rename,
        Action::Cancel,
        Action::Move,
        Action::Rotate,
        Action::Scale,
        Action::Frame,
        Action::Play,
        Action::Pause,
        Action::Step,
        Action::Stop,
        Action::Search,
        Action::Shortcuts,
    ];

    /// Its name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Action::New => "new",
            Action::Open => "open",
            Action::Save => "save",
            Action::SaveAs => "save_as",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::Duplicate => "duplicate",
            Action::Delete => "delete",
            Action::Rename => "rename",
            Action::Cancel => "cancel",
            Action::Move => "move",
            Action::Rotate => "rotate",
            Action::Scale => "scale",
            Action::Frame => "frame",
            Action::Play => "play",
            Action::Pause => "pause",
            Action::Step => "step",
            Action::Stop => "stop",
            Action::Search => "search",
            Action::Shortcuts => "shortcuts",
        }
    }

    /// What it does, for the shortcuts list.
    pub fn describe(self) -> &'static str {
        match self {
            Action::New => "New scene",
            Action::Open => "Open a scene or prefab",
            Action::Save => "Save",
            Action::SaveAs => "Save as",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Duplicate => "Duplicate the selection",
            Action::Delete => "Delete the selection",
            Action::Rename => "Rename the selection",
            Action::Cancel => "Stop placing a prefab, close a dialog",
            Action::Move => "Move gizmo",
            Action::Rotate => "Rotate gizmo",
            Action::Scale => "Scale gizmo",
            Action::Frame => "Frame the selection (3D)",
            Action::Play => "Play (or resume)",
            Action::Pause => "Pause while playing",
            Action::Step => "Step one tick",
            Action::Stop => "Stop: put the world back",
            Action::Search => "Search the assets",
            Action::Shortcuts => "Show the shortcuts",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The keys it has before anything is rebound.
    pub fn defaults(self) -> Vec<KeyboardShortcut> {
        let c = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let none = Modifiers::NONE;
        let k = |m, key| KeyboardShortcut::new(m, key);
        match self {
            Action::New => vec![k(c, Key::N)],
            Action::Open => vec![k(c, Key::O)],
            Action::Save => vec![k(c, Key::S)],
            Action::SaveAs => vec![k(cs, Key::S)],
            Action::Undo => vec![k(c, Key::Z)],
            Action::Redo => vec![k(cs, Key::Z), k(c, Key::Y)],
            Action::Duplicate => vec![k(c, Key::D)],
            Action::Delete => vec![k(none, Key::Delete), k(none, Key::Backspace)],
            Action::Rename => vec![k(none, Key::F2)],
            Action::Cancel => vec![k(none, Key::Escape)],
            Action::Move => vec![k(none, Key::W)],
            Action::Rotate => vec![k(none, Key::E)],
            Action::Scale => vec![k(none, Key::R)],
            Action::Frame => vec![k(none, Key::F)],
            Action::Play => vec![k(c, Key::P)],
            Action::Pause => vec![k(cs, Key::P)],
            Action::Step => vec![k(none, Key::F10)],
            Action::Stop => vec![k(c, Key::Period)],
            Action::Search => vec![k(c, Key::F)],
            Action::Shortcuts => vec![k(none, Key::F1)],
        }
    }
}

/// The keys bound to each action.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Keymap {
    /// (action, its keys), in [`Action::ALL`]'s order.
    bindings: Vec<(Action, Vec<KeyboardShortcut>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: Action::ALL.into_iter().map(|a| (a, a.defaults())).collect(),
        }
    }
}

impl Keymap {
    /// The keys bound to `action`.
    pub fn keys(&self, action: Action) -> &[KeyboardShortcut] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, keys)| keys.as_slice())
    }

    /// Binds `action` to `keys` only (none: it has no shortcut).
    pub fn set(&mut self, action: Action, keys: Vec<KeyboardShortcut>) {
        if let Some((_, k)) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
            *k = keys;
        }
    }

    /// Whether one of `action`'s keys was pressed this frame, with exactly
    /// its Ctrl/⌘, Shift and Alt (so Ctrl+Z doesn't also fire for
    /// Ctrl+Shift+Z).
    pub fn pressed(&self, i: &egui::InputState, action: Action) -> bool {
        self.keys(action).iter().any(|s| {
            i.key_pressed(s.logical_key)
                && i.modifiers.command == s.modifiers.command
                && i.modifiers.shift == s.modifiers.shift
                && i.modifiers.alt == s.modifiers.alt
        })
    }

    /// The other actions already bound to `shortcut`.
    pub fn bound_to(&self, shortcut: &KeyboardShortcut, except: Action) -> Vec<Action> {
        self.bindings
            .iter()
            .filter(|(a, keys)| *a != except && keys.contains(shortcut))
            .map(|(a, _)| *a)
            .collect()
    }

    /// The bindings that differ from the defaults, as the settings file keeps
    /// them: `{"save": ["Ctrl+S"]}`.
    pub fn changes(&self) -> serde_json::Map<String, serde_json::Value> {
        self.bindings
            .iter()
            .filter(|(a, keys)| *keys != a.defaults())
            .map(|(a, keys)| {
                (
                    a.id().to_string(),
                    keys.iter().map(|k| serde_json::Value::String(text(k))).collect(),
                )
            })
            .collect()
    }

    /// The defaults with the bindings of `changes` (see [`changes`]); names
    /// it doesn't know are skipped, and returned as warnings.
    pub fn with_changes(changes: &serde_json::Map<String, serde_json::Value>) -> (Self, Vec<String>) {
        let mut map = Self::default();
        let mut warnings = Vec::new();
        for (id, keys) in changes {
            let Some(action) = Action::from_id(id) else {
                warnings.push(format!("no editor action \"{id}\""));
                continue;
            };
            let mut parsed = Vec::new();
            for key in keys.as_array().into_iter().flatten() {
                match key.as_str().map(parse) {
                    Some(Ok(k)) => parsed.push(k),
                    Some(Err(e)) => warnings.push(format!("{id}: {e}")),
                    None => warnings.push(format!("{id}: a key is a string like \"Ctrl+S\"")),
                }
            }
            map.set(action, parsed);
        }
        (map, warnings)
    }
}

/// A shortcut as the settings file and the shortcuts list write it:
/// `Ctrl+Shift+Z` (Ctrl is ⌘ on macOS), `F2`, `Delete`.
pub(crate) fn text(s: &KeyboardShortcut) -> String {
    let mut out = String::new();
    if s.modifiers.command {
        out.push_str("Ctrl+");
    }
    if s.modifiers.alt {
        out.push_str("Alt+");
    }
    if s.modifiers.shift {
        out.push_str("Shift+");
    }
    out.push_str(s.logical_key.name());
    out
}

/// Reads a shortcut written as [`text`] writes it.
pub(crate) fn parse(text: &str) -> Result<KeyboardShortcut, String> {
    let mut modifiers = Modifiers::NONE;
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().ok_or("an empty shortcut")?;
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => modifiers.command = true,
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            other => {
                return Err(format!(
                    "unknown modifier \"{other}\" in \"{text}\" (use Ctrl, Shift or Alt)"
                ));
            }
        }
    }
    let key = Key::from_name(key).ok_or_else(|| format!("unknown key \"{key}\" in \"{text}\""))?;
    Ok(KeyboardShortcut::new(modifiers, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_written_and_read_back() {
        for action in Action::ALL {
            for key in action.defaults() {
                assert_eq!(parse(&text(&key)), Ok(key), "{action:?}");
            }
        }
        assert_eq!(text(&Action::Redo.defaults()[0]), "Ctrl+Shift+Z");
        assert_eq!(
            parse("cmd + shift + s"),
            Ok(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S))
        );
        assert!(parse("Hyper+S").unwrap_err().contains("unknown modifier"));
        assert!(parse("Ctrl+Banana").unwrap_err().contains("unknown key"));
    }

    #[test]
    fn only_changed_bindings_are_saved_and_unknown_names_are_reported() {
        let mut map = Keymap::default();
        assert!(map.changes().is_empty());
        map.set(Action::Duplicate, vec![parse("Ctrl+J").unwrap()]);
        let changes = map.changes();
        assert_eq!(
            serde_json::Value::Object(changes.clone()),
            serde_json::json!({ "duplicate": ["Ctrl+J"] })
        );
        let (back, warnings) = Keymap::with_changes(&changes);
        assert_eq!(back, map);
        assert!(warnings.is_empty());
        let odd = serde_json::json!({ "fly": ["W"], "save": ["Ctrl+Nope"] });
        let (back, warnings) = Keymap::with_changes(odd.as_object().unwrap());
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            back.keys(Action::Save).is_empty(),
            "a bad key leaves the action unbound, not the default"
        );
    }

    #[test]
    fn a_shortcut_matches_its_modifiers_exactly() {
        let map = Keymap::default();
        let press = |key, modifiers| {
            let egui = egui::Context::default();
            let mut hit = (false, false);
            let input = egui::RawInput {
                events: vec![
                    egui::Event::ModifiersChanged(modifiers),
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                ],
                ..Default::default()
            };
            let mut out = egui.run_ui(input, |ui| {
                hit = ui.input(|i| (map.pressed(i, Action::Undo), map.pressed(i, Action::Redo)));
            });
            out.textures_delta.clear();
            hit
        };
        assert_eq!(press(Key::Z, Modifiers::COMMAND), (true, false));
        assert_eq!(press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), (false, true));
        assert_eq!(press(Key::Y, Modifiers::COMMAND), (false, true));
        assert_eq!(press(Key::Z, Modifiers::NONE), (false, false));
    }

    #[test]
    fn a_conflicting_binding_is_found() {
        let map = Keymap::default();
        let ctrl_d = parse("Ctrl+D").unwrap();
        assert_eq!(map.bound_to(&ctrl_d, Action::Save), vec![Action::Duplicate]);
        assert!(map.bound_to(&ctrl_d, Action::Duplicate).is_empty());
    }
}
