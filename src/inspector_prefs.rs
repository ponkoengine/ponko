//! The editor app's remembered settings in use: the layout and recent
//! files kept in `editor.json` (see `inspector_settings.rs`), the Open
//! recent menu, and the Shortcuts window that lists every shortcut and
//! rebinds it.

use egui::{Color32, RichText, Ui};

use super::Panels;
use super::keys::{self, Action};
use super::settings::{Layout, Settings};
use crate::ctx::Ctx;

/// The Shortcuts window while it's open.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ShortcutsWindow {
    /// The action whose new key the next key press is.
    pub capturing: Option<Action>,
}

/// What the editor app remembers between runs, and what it last saved.
#[derive(Debug, Default)]
pub(crate) struct Prefs {
    pub settings: Settings,
    /// The game's folder: the recent lists are kept per project.
    pub project: String,
    /// The document file last put in the recent list.
    recorded: Option<String>,
    /// The layout as last saved.
    saved_layout: Layout,
    pub shortcuts: Option<ShortcutsWindow>,
}

impl Prefs {
    /// Reads the settings file (`None`: nothing is remembered) for the
    /// game in folder `project`.
    pub fn load(path: Option<std::path::PathBuf>, project: String) -> Self {
        let settings = Settings::load(path);
        Self {
            saved_layout: settings.layout,
            settings,
            project,
            ..Self::default()
        }
    }

    /// Writes the settings, saying why not if that failed.
    fn save(&mut self) -> Option<String> {
        self.saved_layout = self.settings.layout;
        self.settings
            .save(&self.project)
            .err()
            .map(|e| format!("settings not saved: {e}"))
    }
}

impl Panels {
    fn prefs(&mut self) -> Option<&mut Prefs> {
        self.app.as_mut().map(|a| &mut a.prefs)
    }

    /// The first frame: the rebound shortcuts take effect, the project goes
    /// first in the recent list, and problems with the file are shown.
    pub(super) fn start_prefs(&mut self) {
        let Some(prefs) = self.prefs() else { return };
        let keys = prefs.settings.keys.clone();
        let mut warnings = std::mem::take(&mut prefs.settings.warnings);
        let project = prefs.project.clone();
        prefs.settings.opened(&project, None);
        warnings.extend(prefs.save());
        self.keys = keys;
        if !warnings.is_empty() {
            self.error = Some(warnings.join("; "));
        }
    }

    /// Every frame: a document saved or opened goes first in the recent
    /// files, and a layout that changed (and isn't being dragged) is
    /// saved.
    pub(super) fn remember(&mut self, ui: &Ui, ctx: &Ctx) {
        let dragging = ui.input(|i| i.pointer.any_down());
        let path = ctx.editor.doc.path.clone();
        let Some(prefs) = self.prefs() else { return };
        let mut save = false;
        if path.is_some() && path != prefs.recorded {
            prefs.recorded = path.clone();
            let project = prefs.project.clone();
            prefs.settings.opened(&project, path.as_deref());
            save = true;
        }
        if !dragging && prefs.settings.layout != prefs.saved_layout {
            save = true;
        }
        if save && let Some(e) = prefs.save() {
            self.error = Some(e);
        }
    }

    /// The layout the panels open with.
    pub(super) fn layout(&self) -> Layout {
        self.app.as_ref().map(|a| a.prefs.settings.layout).unwrap_or_default()
    }

    pub(super) fn set_layout(&mut self, layout: Layout) {
        if let Some(prefs) = self.prefs() {
            prefs.settings.layout = layout;
        }
    }

    /// This project's recent scene and prefab files, most recent first.
    pub(super) fn recent_files(&self) -> Vec<String> {
        self.app
            .as_ref()
            .map_or_else(Vec::new, |a| a.prefs.settings.recent_files(&a.prefs.project).to_vec())
    }

    /// Whether the Shortcuts window is waiting for a key: no shortcut runs.
    pub(crate) fn capturing_key(&self) -> bool {
        self.app
            .as_ref()
            .and_then(|a| a.prefs.shortcuts.as_ref())
            .is_some_and(|w| w.capturing.is_some())
    }

    pub(crate) fn toggle_shortcuts(&mut self) {
        if let Some(prefs) = self.prefs() {
            prefs.shortcuts = match prefs.shortcuts {
                Some(_) => None,
                None => Some(ShortcutsWindow::default()),
            };
        }
    }

    /// Binds `action` to `keys` only, and saves it.
    pub(crate) fn rebind(&mut self, action: Action, keys: Vec<egui::KeyboardShortcut>) {
        self.keys.set(action, keys);
        let map = self.keys.clone();
        let Some(prefs) = self.prefs() else { return };
        prefs.settings.keys = map;
        if let Some(e) = prefs.save() {
            self.error = Some(e);
        }
    }

    /// Every shortcut, what it does, and buttons to set and reset it. A key
    /// bound to two actions is shown in red on both.
    pub(super) fn shortcuts_window(&mut self, ui: &Ui) {
        let Some(window) = self.app.as_ref().and_then(|a| a.prefs.shortcuts.clone()) else {
            return;
        };
        if let Some(action) = window.capturing {
            // The next key (with its modifiers) is the new shortcut; Esc
            // leaves it as it was.
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } => Some(egui::KeyboardShortcut::new(*modifiers, *key)),
                    _ => None,
                })
            });
            if let Some(shortcut) = pressed {
                // Taken: nothing else this frame sees the key.
                ui.ctx()
                    .input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
                if shortcut.logical_key != egui::Key::Escape || !shortcut.modifiers.is_none() {
                    self.rebind(action, vec![shortcut]);
                }
                if let Some(prefs) = self.prefs() {
                    prefs.shortcuts = Some(ShortcutsWindow { capturing: None });
                }
                return;
            }
        }
        let mut open = true;
        let mut capture = window.capturing;
        let mut reset = None;
        let mut reset_all = false;
        egui::Window::new("Shortcuts")
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ui.ctx().content_rect().center())
            .show(ui.ctx(), |ui| {
                ui.label(
                    RichText::new("Ctrl is Cmd on macOS. Camera flying (right mouse with W A S D Q E) is fixed.")
                        .weak(),
                );
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("ponko-shortcuts")
                        .striped(true)
                        .num_columns(3)
                        .show(ui, |ui| {
                            for action in Action::ALL {
                                ui.label(action.describe());
                                let bound = self.keys.keys(action);
                                let conflict = bound.iter().any(|k| !self.keys.bound_to(k, action).is_empty());
                                let text = if capture == Some(action) {
                                    "press a key… (Esc: keep)".to_string()
                                } else if bound.is_empty() {
                                    "—".to_string()
                                } else {
                                    bound.iter().map(keys::text).collect::<Vec<_>>().join(", ")
                                };
                                let mut label = RichText::new(text).monospace();
                                if conflict {
                                    label = label.color(Color32::LIGHT_RED);
                                }
                                let shown = ui.label(label);
                                if conflict {
                                    let others: Vec<_> = bound
                                        .iter()
                                        .flat_map(|k| self.keys.bound_to(k, action))
                                        .map(Action::describe)
                                        .collect();
                                    shown.on_hover_text(format!("Also: {}", others.join(", ")));
                                }
                                ui.horizontal(|ui| {
                                    if ui.small_button("Set").clicked() {
                                        capture = Some(action);
                                    }
                                    if ui
                                        .add_enabled(bound != action.defaults(), egui::Button::new("Reset").small())
                                        .clicked()
                                    {
                                        reset = Some(action);
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
                if ui.button("Reset all").clicked() {
                    reset_all = true;
                }
            });
        if let Some(action) = reset {
            self.rebind(action, action.defaults());
        }
        if reset_all {
            for action in Action::ALL {
                self.rebind(action, action.defaults());
            }
        }
        if let Some(prefs) = self.prefs() {
            prefs.shortcuts = open.then_some(ShortcutsWindow { capturing: capture });
        }
    }

    /// File > Open recent: this project's recent files.
    pub(super) fn recent_menu(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let files = self.recent_files();
        ui.add_enabled_ui(!files.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for file in files {
                    if ui.button(&file).clicked() {
                        if ctx.editor.dirty() {
                            self.set_dialog(Some(super::app::Dialog::Open { path: file }));
                        } else {
                            self.open_file(ctx, &file);
                        }
                        ui.close();
                    }
                }
            });
        });
    }
}
