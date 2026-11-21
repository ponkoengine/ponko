//! Edit mode: the game started as an editor. `ponko editor <project>`
//! builds the game with the `editor` feature and runs it with
//! `PONKO_EDITOR` set; the game runs its setup, pauses, and its window
//! becomes the editor: a menu bar on top, the hierarchy on the left, the
//! selected entity's components on the right, a status bar below, and the
//! world in the middle as the viewport, with the gizmos.
//!
//! The editor is the game binary itself, not a separate program attached
//! through the control endpoint: only the game's own build knows its
//! component types (reflection), systems (Play), assets and rendering, so
//! the world in the viewport is drawn exactly as the game draws it, and
//! Play runs the real game. The control endpoint's `editor.*` methods make
//! the same edits through the same history, so an agent can work beside a
//! person.
//!
//! These panels are thin: every change goes through `crate::editor` (undo,
//! the document, saving), tested without a window; the panels only lay it
//! out. They're egui, like the inspector they extend.

use egui::{Button, Color32, Key, RichText, ScrollArea, TextEdit, Ui};

use super::Panels;
use super::file_picker::{self, FilePicker};
use super::keys::Action;
use crate::ctx::Ctx;
use crate::editor::{DiskChange, DocKind};
use crate::reflect::WorldReflect;

/// Environment variable that starts the game in edit mode: `1` edits the
/// world the game's setup made; anything else is a scene or prefab file in
/// `assets/` to open. Web builds read the page's `?editor` instead.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const ENV_EDITOR: &str = "PONKO_EDITOR";

/// Whether the game was asked to start in edit mode: `None` if not;
/// `Some(None)` to edit what setup makes; `Some(Some(file))` to open
/// `file`. In web builds the page's `?editor` (`=1`, `=file`) instead.
pub(crate) fn requested() -> Option<Option<String>> {
    #[cfg(target_arch = "wasm32")]
    let setting = crate::web::query_param("editor").map(|v| if v.is_empty() { "1".into() } else { v });
    #[cfg(not(target_arch = "wasm32"))]
    let setting = std::env::var(ENV_EDITOR).ok();
    parse(setting.as_deref())
}

/// [`requested`] from the variable's value.
fn parse(setting: Option<&str>) -> Option<Option<String>> {
    match setting.map(str::trim) {
        None | Some("" | "0") => None,
        Some("1") => Some(None),
        Some(file) => Some(Some(file.to_string())),
    }
}

/// A dialog over the editor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Dialog {
    /// Confirm dropping edits before choosing a file.
    PickOpen,
    /// Open a scene or prefab file with a path already known or entered in the editor.
    Open { path: String },
    /// Save the document as a file (and kind); `then_quit` closes the
    /// editor once saved.
    SaveAs {
        path: String,
        kind: DocKind,
        then_quit: bool,
    },
    /// Save the selected entity (with what hangs off it) as a prefab file;
    /// the document stays what it is.
    SavePrefab { path: String },
    /// Start an empty scene, dropping unsaved changes.
    New,
    /// The window was asked to close with unsaved changes.
    Close,
    /// Stop, with edits made while playing: which to keep.
    Stop {
        keep: Vec<(crate::editor::play::PlayEdit, bool)>,
    },
}

/// The editor's state beyond the inspector's panels.
#[derive(Debug, Default)]
pub(crate) struct App {
    /// What to open once setup has run (`None`: the world as setup made it).
    start: Option<Option<String>>,
    pub dialog: Option<Dialog>,
    pub(super) file_picker: Option<FilePicker>,
    /// Whether Open and Save as use the system's file dialog; else they
    /// ask for the path in a field (web builds, and tests of them).
    pub(super) system_picker: bool,
    /// The window title set last, so it's only set when it changes.
    title: String,
    /// The game's own window title, read on the first frame (the window's
    /// title is the editor's after that).
    game: String,
    /// The layout, recent files and shortcuts kept between runs.
    pub prefs: super::prefs::Prefs,
    /// The asset panel along the bottom.
    pub assets: super::assets_panel::AssetPanel,
    /// The selected prefab instance compared with its prefab.
    pub prefab: Option<super::prefab_panel::PrefabView>,
    /// The selected node's glTF scene compared with its file.
    #[cfg(feature = "3d")]
    pub gltf: Option<super::gltf_panel::GltfView>,
    /// An open prefab document's file as opened (or last applied), for
    /// Apply to scenes.
    pub prefab_base: Option<(String, serde_json::Value)>,
    /// The part of the window the panels leave free, and what the 2D and
    /// 3D cameras were given of it last (see `centre_cameras`).
    pub view: Option<crate::camera::Viewport>,
    view2d: Option<crate::camera::Viewport>,
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    view3d: Option<crate::camera::Viewport>,
}

impl App {
    /// Edit mode for a game started with `start` (see [`requested`]), with
    /// the user's settings file.
    pub fn new(start: Option<String>) -> Self {
        Self::with_settings(
            start,
            super::settings::default_path(),
            super::settings::current_project(),
        )
    }

    /// [`App::new`] with settings file `settings` (`None`: nothing is
    /// remembered) for the game in folder `project`.
    pub fn with_settings(start: Option<String>, settings: Option<std::path::PathBuf>, project: String) -> Self {
        Self {
            start: Some(start),
            prefs: super::prefs::Prefs::load(settings, project),
            system_picker: file_picker::ENABLED,
            ..Self::default()
        }
    }
}

/// The window title: the document's name with `*` when it has unsaved
/// changes, the game's title, and that this is the editor.
pub(crate) fn title(ctx: &Ctx, game: &str) -> String {
    let doc = &ctx.editor.doc;
    let star = if ctx.editor.dirty() { "*" } else { "" };
    format!("{}{star} — {game} (editor)", doc.name())
}

impl Panels {
    /// The editor's whole window, one frame.
    pub(crate) fn show_app(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        self.start_app(ctx);
        self.begin_frame(ui, ctx);
        self.watch_disk(ctx);
        self.close_request(ctx);
        if !self.capturing_key() {
            self.app_shortcuts(ui, ctx);
            if self.dialog().is_none() {
                self.play_keys(ui, ctx);
            }
        }
        let mut layout = self.layout();

        egui::Panel::top("ponko-editor-bar").show(ui, |ui| self.menu_bar(ui, ctx));
        egui::Panel::bottom("ponko-editor-status").show(ui, |ui| self.status_bar(ui, ctx));
        if layout.assets {
            layout.bottom = egui::Panel::bottom("ponko-editor-assets")
                .default_size(layout.bottom)
                .resizable(true)
                .show(ui, |ui| self.assets_panel(ui, ctx))
                .response
                .rect
                .height();
        }
        layout.left = egui::Panel::left("ponko-editor-hierarchy")
            .default_size(layout.left)
            .resizable(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Hierarchy");
                    if ui
                        .small_button("+ entity")
                        .on_hover_text("An entity with only a transform, in the middle of the view")
                        .clicked()
                    {
                        self.new_entity(ctx);
                    }
                });
                let height = ui.available_height();
                self.hierarchy_rows(ui, ctx, height);
            })
            .response
            .rect
            .width();
        layout.right = egui::Panel::right("ponko-editor-inspector")
            .default_size(layout.right)
            .resizable(true)
            .show(ui, |ui| {
                ScrollArea::vertical().id_salt("editor-inspector").show(ui, |ui| {
                    self.inspector_column(ui, ctx);
                });
            })
            .response
            .rect
            .width();
        self.set_layout(layout);
        // Not while a button is held: a drag in the view keeps the view it
        // started in, however the panels' contents change meanwhile.
        if !ui.input(|i| i.pointer.any_down()) {
            let free = ui.available_rect_before_wrap();
            self.centre_cameras(ctx, free, ui.ctx().content_rect());
        }
        self.dialogs(ui, ctx);
        self.shortcuts_window(ui);
        self.world_and_keys(ui, ctx);
        self.remember(ui, ctx);
        self.remember_prefab_base(ctx);

        if let Some(app) = &mut self.app
            && let title = title(ctx, &app.game)
            && app.title != title
        {
            // Headless (tests) there's no window: the title is kept anyway.
            let _ = ctx.window.set_title(title.clone());
            app.title = title;
        }
    }

    /// Gives the 2D and 3D cameras the part of the window the panels leave
    /// free (`free`, of the window `screen`), so they centre on it: what
    /// was in the middle of the 2D view stays in the middle of the free
    /// part, the 3D camera's middle is the free part's middle, and their
    /// conversions, picking and the 3D aspect ratio follow it. A camera the
    /// game gave a part of the window itself (split screen) is left alone.
    fn centre_cameras(&mut self, ctx: &mut Ctx, free: egui::Rect, screen: egui::Rect) {
        use crate::camera::Viewport;
        let Some(app) = &mut self.app else { return };
        if free.width() < 1.0 || free.height() < 1.0 || screen.width() < 1.0 || screen.height() < 1.0 {
            return;
        }
        let view = Viewport::new(
            (free.min.x - screen.min.x) / screen.width(),
            (free.min.y - screen.min.y) / screen.height(),
            free.width() / screen.width(),
            free.height() / screen.height(),
        );
        app.view = Some(view);
        let ours = |rect: Viewport, set: Option<Viewport>| rect.is_full() || set == Some(rect);
        let camera = &mut ctx.camera;
        if ours(camera.rect, app.view2d) {
            if camera.rect != view {
                let middle = camera.center();
                camera.rect = view;
                camera.look_at(middle);
            }
            app.view2d = Some(view);
        }
        #[cfg(feature = "3d")]
        {
            let camera = &mut ctx.scene3d.camera;
            if ours(camera.rect, app.view3d) {
                camera.rect = view;
                app.view3d = Some(view);
            }
        }
    }

    /// The first frame after setup: pause, and open the file asked for (or
    /// make the world setup built the document).
    fn start_app(&mut self, ctx: &mut Ctx) {
        let Some(start) = self.app.as_mut().and_then(|a| a.start.take()) else {
            return;
        };
        ctx.time.pause();
        if let Some(app) = &mut self.app {
            app.game = ctx.window.settings().title.clone();
        }
        self.start_prefs();
        match start {
            Some(path) => self.open_file(ctx, &path),
            None => {
                // Takes in the scene setup loaded, if any, as the document.
                ctx.edit(|_, _| ());
            }
        }
    }

    /// Looks at the document's file on disk now and then (see
    /// `Editor::check_disk`).
    fn watch_disk(&mut self, ctx: &mut Ctx) {
        let now = web_time::Instant::now();
        match ctx.edit(|ed, ctx| ed.check_disk(ctx, now)) {
            Some(DiskChange::Reloaded) => {
                self.status = Some(format!("{} changed on disk: opened again", ctx.editor.doc.name()));
                self.select(None, false);
            }
            Some(DiskChange::Conflict) | None => {}
        }
    }

    /// Closing the window with unsaved changes asks first.
    fn close_request(&mut self, ctx: &mut Ctx) {
        if ctx.window.close_requested() && ctx.editor.dirty() && !ctx.quit_requested() {
            ctx.window.cancel_close();
            self.set_dialog(Some(Dialog::Close));
        }
    }

    pub(super) fn set_dialog(&mut self, dialog: Option<Dialog>) {
        if let Some(app) = &mut self.app {
            app.dialog = dialog;
        }
    }

    fn dialog(&self) -> Option<&Dialog> {
        self.app.as_ref().and_then(|a| a.dialog.as_ref())
    }

    /// Ctrl+S saves, Ctrl+Shift+S saves as, Ctrl+O opens, Ctrl+N starts a
    /// new scene (⌘ on macOS), F1 lists the shortcuts; unless rebound.
    fn app_shortcuts(&mut self, ui: &Ui, ctx: &mut Ctx) {
        if self.dialog().is_some() {
            return;
        }
        let (save, save_as, open, new, help) = ui.input(|i| {
            use super::keys::Action;
            (
                self.keys.pressed(i, Action::Save),
                self.keys.pressed(i, Action::SaveAs),
                self.keys.pressed(i, Action::Open),
                self.keys.pressed(i, Action::New),
                self.keys.pressed(i, Action::Shortcuts),
            )
        });
        if help {
            self.toggle_shortcuts();
        }
        if save {
            self.save(ctx);
        } else if save_as {
            self.ask_save_as(ctx, false);
        } else if open {
            self.ask_open(ctx);
        } else if new {
            self.ask_new(ctx);
        }
    }

    /// Saves the document to its file, or asks for one.
    pub(crate) fn save(&mut self, ctx: &mut Ctx) -> bool {
        if ctx.editor.doc.path.is_none() {
            self.ask_save_as(ctx, false);
            return false;
        }
        let saved = ctx.edit(|ed, ctx| ed.save(ctx)).map_err(String::from);
        match self.report(saved) {
            Some(path) => {
                self.status = Some(format!("saved {path}"));
                self.scene.forget_assets();
                self.prefabs = None;
                true
            }
            None => false,
        }
    }

    fn ask_save_as(&mut self, ctx: &Ctx, then_quit: bool) {
        let doc = &ctx.editor.doc;
        let path = doc.path.clone().unwrap_or_else(|| super::DEFAULT_SCENE.to_string());
        self.set_dialog(Some(Dialog::SaveAs {
            path,
            kind: doc.kind,
            then_quit,
        }));
    }

    /// Whether Open and Save as use the system's file dialog (see
    /// `App::system_picker`).
    fn system_picker(&self) -> bool {
        self.app.as_ref().is_some_and(|app| app.system_picker)
    }

    fn ask_open(&mut self, ctx: &Ctx) {
        if !self.system_picker() {
            let path = ctx.editor.doc.path.clone().unwrap_or_default();
            self.set_dialog(Some(Dialog::Open { path }));
        } else if ctx.editor.dirty() {
            self.set_dialog(Some(Dialog::PickOpen));
        } else {
            self.pick_file(FilePicker::Open);
        }
    }

    fn ask_new(&mut self, ctx: &mut Ctx) {
        if ctx.editor.dirty() {
            self.set_dialog(Some(Dialog::New));
        } else {
            self.new_scene(ctx);
        }
    }

    fn new_scene(&mut self, ctx: &mut Ctx) {
        let made = ctx.edit(|ed, ctx| ed.new_scene(ctx)).map_err(String::from);
        if self.report(made).is_some() {
            self.status = Some("new scene".into());
            self.select(None, false);
        }
    }

    fn new_entity(&mut self, ctx: &mut Ctx) {
        #[cfg(feature = "3d")]
        if self.scene.active3d {
            let camera = ctx.scene3d.camera;
            let at = camera.position + camera.forward() * 10.0;
            let e = ctx.edit(|ed, ctx| ed.spawn_empty3d(ctx, crate::d3::Transform::at(at)));
            self.select(Some(e), false);
            return;
        }
        let at = ctx.camera.center();
        let e = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, at));
        self.select(Some(e), false);
    }

    /// File and Edit menus, Play / Stop, the gizmo's settings, the view.
    fn menu_bar(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button(self.labelled("New scene", Action::New)).clicked() {
                    self.ask_new(ctx);
                    ui.close();
                }
                if ui.button(self.labelled("Open…", Action::Open)).clicked() {
                    self.ask_open(ctx);
                    ui.close();
                }
                self.recent_menu(ui, ctx);
                if ui.button(self.labelled("Save", Action::Save)).clicked() {
                    self.save(ctx);
                    ui.close();
                }
                if ui.button(self.labelled("Save as…", Action::SaveAs)).clicked() {
                    self.ask_save_as(ctx, false);
                    ui.close();
                }
                let selected = self.selected.is_some();
                if ui
                    .add_enabled(selected, Button::new("Save selection as prefab…"))
                    .clicked()
                {
                    self.set_dialog(Some(Dialog::SavePrefab {
                        path: format!("{}/thing.json", super::PREFAB_FOLDER),
                    }));
                    ui.close();
                }
                ui.separator();
                let has_file = ctx.editor.doc.path.is_some();
                if ui
                    .add_enabled(has_file, Button::new("Reload from disk"))
                    .on_hover_text("Open the file again, dropping unsaved changes")
                    .clicked()
                {
                    self.reload(ctx);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(ctx.editor.can_undo(), Button::new(self.labelled("Undo", Action::Undo)))
                    .clicked()
                {
                    self.undo(ctx);
                    ui.close();
                }
                if ui
                    .add_enabled(ctx.editor.can_redo(), Button::new(self.labelled("Redo", Action::Redo)))
                    .clicked()
                {
                    let redone = ctx.edit(|ed, ctx| ed.redo(ctx));
                    self.report(redone);
                    ui.close();
                }
                ui.separator();
                let selection = self.selection();
                if ui
                    .add_enabled(
                        !selection.is_empty(),
                        Button::new(self.labelled("Duplicate", Action::Duplicate)),
                    )
                    .clicked()
                {
                    self.duplicate_selection(ctx);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        selection.len() == 1,
                        Button::new(self.labelled("Rename", Action::Rename)),
                    )
                    .clicked()
                {
                    if let Some(e) = self.selected {
                        self.start_rename(ctx, e);
                    }
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !selection.is_empty(),
                        Button::new(self.labelled("Delete", Action::Delete)),
                    )
                    .clicked()
                {
                    let deleted = ctx.edit(|ed, ctx| ed.delete_many(ctx, &selection));
                    self.report(deleted);
                    self.select(None, false);
                    ui.close();
                }
                if ui.button("New entity").clicked() {
                    self.new_entity(ctx);
                    ui.close();
                }
            });
            ui.menu_button("Window", |ui| {
                let mut layout = self.layout();
                if ui.checkbox(&mut layout.assets, "Assets panel").changed() {
                    self.set_layout(layout);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button(self.labelled("Shortcuts…", Action::Shortcuts)).clicked() {
                    self.toggle_shortcuts();
                    ui.close();
                }
            });
            ui.separator();
            self.play_bar(ui, ctx);
            ui.separator();
            self.view_toggle(ui);
        });
        self.gizmo_tools(ui);
    }

    /// A menu entry with its shortcut, as bound now: "Save  (Ctrl+S)".
    pub(super) fn labelled(&self, text: &str, action: Action) -> String {
        match self.keys.keys(action).first() {
            Some(key) => format!("{text}  ({})", super::keys::text(key)),
            None => text.to_string(),
        }
    }

    fn reload(&mut self, ctx: &mut Ctx) {
        let reloaded = ctx.edit(|ed, ctx| ed.reload(ctx)).map_err(String::from);
        if self.report(reloaded).is_some() {
            self.status = Some(format!("opened {} again", ctx.editor.doc.name()));
            self.select(None, false);
        }
    }

    /// The document and whether it's saved, what happened last, and the
    /// question when the file changed on disk under unsaved changes.
    fn status_bar(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        if ctx.editor.doc.conflict {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(255, 190, 60),
                    format!(
                        "{} changed on disk, and you have unsaved changes.",
                        ctx.editor.doc.name()
                    ),
                );
                if ui.button("Reload (drop my changes)").clicked() {
                    self.reload(ctx);
                }
                if ui
                    .button("Keep mine")
                    .on_hover_text("The next save writes over the file")
                    .clicked()
                {
                    ctx.edit(|ed, _| ed.keep_mine());
                }
            });
        }
        ui.horizontal(|ui| {
            let doc = &ctx.editor.doc;
            let name = match doc.path {
                // "untitled scene" says the kind already.
                None => doc.name(),
                Some(_) => format!("{} {}", doc.kind.name(), doc.name()),
            };
            ui.label(RichText::new(name).strong());
            if ctx.editor.dirty() {
                ui.colored_label(Color32::from_rgb(255, 190, 60), "unsaved changes");
            } else {
                ui.label(RichText::new("saved").weak());
            }
            if ctx.editor.playing() {
                ui.colored_label(Color32::from_rgb(120, 200, 255), "playing: Stop puts the world back");
            }
            ui.separator();
            if let Some(error) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            } else if let Some(status) = &self.status {
                ui.label(RichText::new(status).weak());
            }
        });
    }

    /// The selected entity's components, then resources, assets, systems.
    fn inspector_column(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let selection = self.selection();
        self.prefab_document(ui, ctx);
        match self.selected {
            Some(e) => {
                if selection.len() > 1 {
                    ui.label(RichText::new(format!("{} selected; showing the first", selection.len())).weak());
                }
                self.prefab_section(ui, ctx, e);
                #[cfg(feature = "3d")]
                self.gltf_section(ui, ctx, e);
                self.entity(ui, ctx, e);
            }
            None => {
                ui.label(RichText::new("Nothing selected: click an entity in the view or the hierarchy").weak());
            }
        }
        ui.separator();
        if !ctx.world.reflected_resources().is_empty() {
            egui::CollapsingHeader::new("Resources")
                .default_open(false)
                .show(ui, |ui| self.resources(ui, ctx));
        }
        egui::CollapsingHeader::new("Performance")
            .default_open(false)
            .show(ui, |ui| {
                self.frame_graph(ui);
                super::systems(ui, ctx);
            });
    }

    /// The open dialog, if any.
    fn dialogs(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(dialog) = self.dialog().cloned() else {
            return;
        };
        let mut current = dialog.clone();
        let mut done = false;
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let dirty = ctx.editor.dirty();
        let system_picker = self.system_picker();
        let title = match &dialog {
            Dialog::PickOpen | Dialog::Open { .. } => "Open",
            Dialog::SaveAs { .. } => "Save as",
            Dialog::SavePrefab { .. } => "Save selection as prefab",
            Dialog::New => "New scene",
            Dialog::Close => "Unsaved changes",
            Dialog::Stop { .. } => "Stop",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| match &mut current {
                Dialog::PickOpen => {
                    ui.label("Unsaved changes will be lost when you open another file.");
                    ui.horizontal(|ui| {
                        if !escape && (ui.button("Choose file…").clicked() || enter) {
                            self.pick_file(FilePicker::Open);
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::Open { path } => {
                    ui.label("A scene or prefab in assets/:");
                    path_field(ui, path);
                    let files = {
                        let assets = self.asset_list(ctx);
                        let mut files = assets.scenes;
                        files.extend(assets.prefabs);
                        files
                    };
                    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        for file in files {
                            if ui.selectable_label(*path == file, &file).clicked() {
                                *path = file;
                            }
                        }
                    });
                    if dirty {
                        ui.colored_label(Color32::from_rgb(255, 190, 60), "Unsaved changes will be lost.");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() || (enter && !path.is_empty()) {
                            let path = path.clone();
                            self.open_file(ctx, &path);
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::SaveAs { path, kind, then_quit } => {
                    if !system_picker {
                        ui.label("File in assets/:");
                        path_field(ui, path);
                    }
                    ui.horizontal(|ui| {
                        ui.radio_value(kind, DocKind::Scene, "Scene")
                            .on_hover_text("The whole world");
                        ui.radio_value(kind, DocKind::Prefab, "Prefab")
                            .on_hover_text("The world's one root entity and what hangs off it");
                    });
                    if !system_picker && exists(path) && ctx.editor.doc.path.as_deref() != Some(path.as_str()) {
                        ui.colored_label(Color32::from_rgb(255, 190, 60), "That file exists: saving replaces it.");
                    }
                    ui.horizontal(|ui| {
                        if system_picker {
                            if !escape && (ui.button("Choose file…").clicked() || enter) {
                                self.pick_file(FilePicker::SaveAs {
                                    kind: *kind,
                                    then_quit: *then_quit,
                                });
                                done = true;
                            }
                        } else if ui.button("Save").clicked() || (enter && !path.is_empty()) {
                            self.save_as(ctx, path, *kind);
                            if self.error.is_none() {
                                done = true;
                                if *then_quit {
                                    ctx.quit();
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::SavePrefab { path } => {
                    ui.label("Prefab file in assets/:");
                    path_field(ui, path);
                    ui.horizontal(|ui| {
                        if (ui.button("Save").clicked() || (enter && !path.is_empty()))
                            && let Some(e) = self.selected
                        {
                            let saved = ctx
                                .assets
                                .save_prefab(&ctx.world, e, path)
                                .map_err(String::from)
                                .map(|()| format!("saved prefab {path}"));
                            if let Some(message) = self.report(saved) {
                                self.status = Some(message);
                                self.scene.forget_assets();
                                self.prefabs = None;
                                done = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::New => {
                    ui.label("Start an empty scene? Unsaved changes will be lost.");
                    ui.horizontal(|ui| {
                        if ui.button("New scene").clicked() || enter {
                            self.new_scene(ctx);
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::Stop { keep } => {
                    done = self.stop_dialog(ui, ctx, keep, enter);
                }
                Dialog::Close => {
                    ui.label(format!("Save the changes to {} before closing?", ctx.editor.doc.name()));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() || enter {
                            if ctx.editor.doc.path.is_none() {
                                done = true;
                                self.ask_save_as(ctx, true);
                                return;
                            }
                            if self.save(ctx) {
                                ctx.quit();
                            }
                            done = true;
                        }
                        if ui.button("Don't save").clicked() {
                            ctx.quit();
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
            });
        if escape {
            done = true;
        }
        // A dialog that opened another (Save as from Close) keeps that one.
        if self.dialog() == Some(&dialog) {
            self.set_dialog(if done { None } else { Some(current) });
        }
    }

    /// The asset browser's list (scenes, prefabs, meshes), read once.
    pub(super) fn asset_list(&mut self, ctx: &Ctx) -> crate::editor::AssetList {
        self.scene.asset_list(ctx)
    }
}

/// A one-line path field that has the keyboard while its dialog is open.
fn path_field(ui: &mut Ui, path: &mut String) {
    let field = ui.add(TextEdit::singleline(path).desired_width(280.0));
    if !field.has_focus() && !field.lost_focus() {
        field.request_focus();
    }
}

/// Whether asset file `path` exists.
fn exists(path: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::assets::resolve(path).is_ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Transform as T2;
    use crate::editor::DocKind;
    use glam::Vec2;
    use ponko_ecs::{Component, Entity};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Hp(f32);

    /// An asset file name unique to this test run, removed when dropped.
    struct TempAsset {
        name: String,
        file: std::path::PathBuf,
    }

    impl TempAsset {
        fn new(what: &str) -> Self {
            let name = format!("ponko-test-app-{what}-{}.json", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&name);
            Self { name, file }
        }
    }

    impl TempAsset {
        /// A copy of fixture `fixture` (in `tests/fixtures/cook/source`)
        /// named for this test run, keeping its extension.
        fn copy_of(what: &str, fixture: &str) -> Self {
            let ext = fixture.rsplit('.').next().unwrap();
            let name = format!("ponko-test-app-{what}-{}.{ext}", std::process::id());
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let file = root.join("assets").join(&name);
            std::fs::copy(root.join("tests/fixtures/cook/source").join(fixture), &file).unwrap();
            Self { name, file }
        }
    }

    impl Drop for TempAsset {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
        }
    }

    const SCREEN: egui::Vec2 = egui::vec2(1280.0, 720.0);
    /// Width of the side panels with the default layout.
    const SIDE_PANEL: f32 = 300.0;

    /// A game in edit mode: the world its setup made, the panels as the
    /// editor, and a lasting egui context. One frame has run.
    fn editor(start: Option<String>) -> (egui::Context, Panels, Ctx) {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SCREEN.x, SCREEN.y));
        ctx.world.reflect::<Hp>();
        let middle = ctx.camera.center();
        ctx.world.spawn((T2::at(middle), Hp(1.0)));
        let mut panels = Panels {
            app: Some(App::new(start)),
            ..Default::default()
        };
        let egui = egui::Context::default();
        frame(&egui, &mut panels, &mut ctx, vec![]);
        (egui, panels, ctx)
    }

    /// Runs one frame of the editor with these input events. Held
    /// modifiers come from the key events.
    fn frame(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, events: Vec<egui::Event>) {
        drawn(egui, p, ctx, events);
    }

    /// [`frame`], returning what it drew.
    fn drawn(
        egui: &egui::Context,
        p: &mut Panels,
        ctx: &mut Ctx,
        mut events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        let held = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .fold(egui::Modifiers::NONE, |a, b| a | b);
        events.insert(0, egui::Event::ModifiersChanged(held));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        let mut out = p.run(egui, input, ctx);
        out.textures_delta.clear();
        out.shapes
    }

    /// The middle of the first text drawn that `wanted` accepts: where a
    /// person would point to grab that row or field.
    fn text_at(shapes: &[egui::epaint::ClippedShape], wanted: impl Fn(&str) -> bool) -> Option<Vec2> {
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

    /// Presses at `from`, moves past egui's drag threshold, on to `to` in
    /// `steps` frames, and lets go there.
    fn mouse_drag(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, from: Vec2, to: Vec2, steps: usize) {
        frame(egui, p, ctx, vec![moved(from)]);
        frame(egui, p, ctx, vec![button(from, true)]);
        for i in 1..=steps {
            frame(egui, p, ctx, vec![moved(from.lerp(to, i as f32 / steps as f32))]);
        }
        frame(egui, p, ctx, vec![button(to, false)]);
        frame(egui, p, ctx, vec![]);
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn command(k: Key) -> egui::Event {
        key(k, egui::Modifiers::COMMAND)
    }

    fn plain(k: Key) -> egui::Event {
        key(k, egui::Modifiers::NONE)
    }

    fn button(at: Vec2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(at.x, at.y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    fn moved(at: Vec2) -> egui::Event {
        egui::Event::PointerMoved(egui::pos2(at.x, at.y))
    }

    // The real window drains queued requests after drawing. Tests supply a
    // path or cancellation through the same entry point.
    fn answer_picker(p: &mut Panels, ctx: &mut Ctx, expected: FilePicker, path: Option<&std::path::Path>) {
        let mut called = false;
        p.run_file_picker(ctx, |request, _, _| {
            called = true;
            assert_eq!(request, expected);
            path.map(std::path::Path::to_path_buf)
        });
        assert!(called, "the editor queued a file picker");
    }

    /// The web build's answer to Open or Save as: types `path` into the
    /// dialog's path field and presses Enter, or cancels with Esc (`None`),
    /// as [`answer_picker`] answers for the system dialog.
    fn answer_path_field(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, path: Option<&str>) {
        assert!(
            matches!(p.dialog(), Some(Dialog::Open { .. } | Dialog::SaveAs { .. })),
            "{:?}",
            p.dialog()
        );
        match path {
            Some(path) => {
                frame(egui, p, ctx, vec![]); // the field takes the keyboard
                let typed = vec![command(Key::A), egui::Event::Text(path.into())];
                frame(egui, p, ctx, typed);
                frame(egui, p, ctx, vec![plain(Key::Enter)]);
            }
            None => frame(egui, p, ctx, vec![plain(Key::Escape)]),
        }
        let asked = p.run_file_picker(ctx, |_, _, _| panic!("the path field, not a system dialog"));
        assert!(!asked);
    }

    /// The editor as the web build has it: no system file dialog.
    fn web_editor() -> (egui::Context, Panels, Ctx) {
        let (egui, mut p, ctx) = editor(None);
        p.app.as_mut().unwrap().system_picker = false;
        (egui, p, ctx)
    }

    /// One frame with exactly these events: unlike [`frame`], held
    /// modifiers change only with a `ModifiersChanged` event, as a window
    /// reports them.
    fn raw_frame(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        p.run(egui, input, ctx).textures_delta.clear();
    }

    /// Whether egui holds any modifier, or Enter, down.
    fn held(egui: &egui::Context) -> bool {
        egui.input(|i| i.modifiers.any() || i.key_down(Key::Enter))
    }

    /// The window title the editor set last.
    fn shown(p: &Panels) -> String {
        p.app.as_ref().unwrap().title.clone()
    }

    fn the_entity(ctx: &mut Ctx) -> Entity {
        ctx.world
            .query::<(Entity, &Hp)>()
            .iter()
            .map(|(e, _)| e)
            .next()
            .unwrap()
    }

    #[test]
    fn the_variable_turns_edit_mode_on_with_or_without_a_file() {
        assert_eq!(parse(None), None);
        assert_eq!(parse(Some("0")), None);
        assert_eq!(parse(Some("")), None);
        assert_eq!(parse(Some("1")), Some(None));
        assert_eq!(parse(Some(" levels/a.json ")), Some(Some("levels/a.json".into())));
    }

    #[test]
    fn edit_mode_starts_paused_and_opens_the_file_it_was_given() {
        let file = TempAsset::new("start");
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SCREEN.x, SCREEN.y));
        ctx.world.reflect::<Hp>();
        ctx.world.spawn((Hp(3.0),));
        ctx.assets.save_scene(&ctx.world, &file.name).unwrap();
        let (_, panels, mut ctx) = editor(Some(file.name.clone()));
        assert!(ctx.time.is_paused() && panels.editing);
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
        let hp: Vec<f32> = ctx.world.query::<&Hp>().iter().map(|h| h.0).collect();
        assert_eq!(hp, [3.0], "the file's world, not the one setup made");
        assert!(panels.error.is_none(), "{:?}", panels.error);
        assert!(shown(&panels).starts_with(&file.name) && !shown(&panels).contains('*'));
    }

    #[test]
    fn a_change_marks_the_title_and_ctrl_s_saves_and_clears_it() {
        let file = TempAsset::new("ctrl-s");
        let (egui, mut p, mut ctx) = editor(None);
        assert!(ctx.editor.doc.path.is_none());
        let e = the_entity(&mut ctx);
        p.select(Some(e), false);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::D)]); // duplicate
        assert!(ctx.editor.dirty());
        assert!(shown(&p).starts_with("untitled scene*"), "{}", shown(&p));

        // Untitled: Ctrl+S asks for the kind, then the picker supplies the filename.
        frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
        assert!(matches!(p.dialog(), Some(Dialog::SaveAs { .. })), "{:?}", p.dialog());
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        answer_picker(
            &mut p,
            &mut ctx,
            FilePicker::SaveAs {
                kind: DocKind::Scene,
                then_quit: false,
            },
            Some(&file.file),
        );
        frame(&egui, &mut p, &mut ctx, vec![]);
        assert!(p.dialog().is_none(), "{:?} {:?}", p.dialog(), p.error);
        assert!(file.file.exists());
        assert!(!ctx.editor.dirty());
        assert!(!shown(&p).contains('*'), "{}", shown(&p));
        assert_eq!(shown(&p).matches("(editor)").count(), 1, "{}", shown(&p));

        // Named: Ctrl+S writes the file again, no question.
        frame(&egui, &mut p, &mut ctx, vec![command(Key::D)]);
        assert!(ctx.editor.dirty());
        frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
        assert!(p.dialog().is_none() && !ctx.editor.dirty());
        let saved = std::fs::read_to_string(&file.file).unwrap();
        assert_eq!(saved.matches("\"Hp\"").count(), 3, "the original and two copies");

        // Undo past the save: unsaved again.
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert!(ctx.editor.dirty());
    }

    #[test]
    fn save_as_prefab_uses_the_picked_file_and_open_reads_it_as_a_prefab() {
        let file = TempAsset::new("as-prefab");
        let (egui, mut p, mut ctx) = editor(None);
        let shortcut = key(Key::S, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        frame(&egui, &mut p, &mut ctx, vec![shortcut]);
        let Some(Dialog::SaveAs { kind, .. }) = p.app.as_mut().unwrap().dialog.as_mut() else {
            panic!("Save as asks for the document kind");
        };
        *kind = DocKind::Prefab;
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        answer_picker(
            &mut p,
            &mut ctx,
            FilePicker::SaveAs {
                kind: DocKind::Prefab,
                then_quit: false,
            },
            Some(&file.file),
        );
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(ctx.editor.doc.kind, DocKind::Prefab);
        assert!(std::fs::read_to_string(&file.file).unwrap().contains("ponko-prefab"));

        frame(&egui, &mut p, &mut ctx, vec![command(Key::N)]);
        assert_eq!(ctx.world.len(), 0);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        answer_picker(&mut p, &mut ctx, FilePicker::Open, Some(&file.file));
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(ctx.editor.doc.kind, DocKind::Prefab);
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
        assert_eq!(ctx.world.len(), 1);
    }

    #[test]
    fn f2_renames_the_selection_in_the_hierarchy_and_undo_takes_it_back() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        p.select(Some(e), false);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::F2)]);
        assert_eq!(p.scene.renaming, Some((e, String::new())));
        frame(&egui, &mut p, &mut ctx, vec![]); // the field takes the keyboard
        frame(&egui, &mut p, &mut ctx, vec![egui::Event::Text("Crab".into())]);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        assert_eq!(p.scene.renaming, None);
        let name = ctx.world.get::<crate::hierarchy::EntityName>(e).map(|n| n.0.clone());
        assert_eq!(name.as_deref(), Some("Crab"));
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert!(ctx.world.get::<crate::hierarchy::EntityName>(e).is_none());
    }

    #[test]
    fn closing_with_unsaved_changes_asks_and_dont_save_quits() {
        let (egui, mut p, mut ctx) = editor(None);
        // Nothing unsaved: the window closes.
        ctx.window.feed_event(&crate::window::WindowEvent::CloseRequested);
        frame(&egui, &mut p, &mut ctx, vec![]);
        assert!(ctx.window.close_requested() && p.dialog().is_none());
        ctx.window.cancel_close();

        let e = the_entity(&mut ctx);
        p.select(Some(e), false);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Delete)]);
        ctx.window.feed_event(&crate::window::WindowEvent::CloseRequested);
        frame(&egui, &mut p, &mut ctx, vec![]);
        assert!(!ctx.window.close_requested(), "vetoed");
        assert_eq!(p.dialog(), Some(&Dialog::Close));
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Escape)]);
        assert!(p.dialog().is_none() && !ctx.quit_requested(), "Esc cancels");
    }

    #[test]
    fn new_asks_before_discarding_changes_and_open_picks_immediately_when_clean() {
        let (egui, mut p, mut ctx) = editor(None);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::N)]);
        assert!(p.dialog().is_none(), "nothing unsaved: a new scene at once");
        assert_eq!(ctx.world.len(), 0);
        let middle = ctx.camera.center();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, middle));
        frame(&egui, &mut p, &mut ctx, vec![command(Key::N)]);
        assert_eq!(p.dialog(), Some(&Dialog::New));
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        assert_eq!(ctx.world.len(), 0);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        assert_eq!(p.dialog(), None);
        answer_picker(&mut p, &mut ctx, FilePicker::Open, None);
    }

    #[test]
    fn save_as_starts_in_the_documents_folder_and_a_name_without_extension_saves_json() {
        let file = TempAsset::new("no-extension");
        let (egui, mut p, mut ctx) = editor(None);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        let typed = file.file.with_extension("");
        p.run_file_picker(&mut ctx, |_, _, name| {
            assert_eq!(name, Some("level.json"), "an untitled scene suggests the default name");
            Some(typed.clone())
        });
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(file.file.is_file(), "the typed name gets .json");
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));

        // Save as again: the picker opens where the document is, with its name.
        let shortcut = key(Key::S, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        frame(&egui, &mut p, &mut ctx, vec![shortcut]);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        p.run_file_picker(&mut ctx, |_, folder, name| {
            assert_eq!(folder, file.file.parent().unwrap());
            assert_eq!(name, Some(file.name.as_str()));
            None
        });
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
    }

    #[test]
    fn canceling_the_open_picker_keeps_unsaved_changes_and_the_document() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        ctx.edit(|ed, ctx| ed.rename(ctx, e, "Unsaved")).unwrap();
        let world = ctx.world.save();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        assert_eq!(p.dialog(), Some(&Dialog::PickOpen));
        p.run_file_picker(&mut ctx, |_, _, _| panic!("ask before opening the picker"));
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        answer_picker(&mut p, &mut ctx, FilePicker::Open, None);
        assert!(p.dialog().is_none() && ctx.editor.dirty() && ctx.editor.doc.path.is_none());
        assert_eq!(ctx.world.save(), world);
    }

    #[test]
    fn open_refuses_a_picked_file_shadowed_by_a_registered_asset_source() {
        let file = TempAsset::new("shadowed-open");
        let (egui, mut p, mut ctx) = editor(None);
        ctx.assets.save_scene(&ctx.world, &file.name).unwrap();
        ctx.assets
            .add_source(crate::assets::MemorySource::new("embedded").with(&file.name, "other data"));

        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        answer_picker(&mut p, &mut ctx, FilePicker::Open, Some(&file.file));
        assert!(p.error.as_deref().unwrap().contains("embedded takes precedence"));
        assert!(ctx.editor.doc.path.is_none());
    }

    #[test]
    fn close_after_save_as_quits_only_after_the_chosen_file_is_saved() {
        let file = TempAsset::new("save-on-close");
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        ctx.edit(|ed, ctx| ed.rename(ctx, e, "Unsaved")).unwrap();

        let request = FilePicker::SaveAs {
            kind: DocKind::Scene,
            then_quit: true,
        };
        let ask = |p: &mut Panels, ctx: &mut Ctx| {
            ctx.window.feed_event(&crate::window::WindowEvent::CloseRequested);
            frame(&egui, p, ctx, vec![]);
            assert_eq!(p.dialog(), Some(&Dialog::Close));
            frame(&egui, p, ctx, vec![plain(Key::Enter)]);
            assert!(matches!(p.dialog(), Some(Dialog::SaveAs { then_quit: true, .. })));
            frame(&egui, p, ctx, vec![plain(Key::Enter)]);
        };

        ask(&mut p, &mut ctx);
        answer_picker(&mut p, &mut ctx, request, None);
        assert!(!ctx.quit_requested() && ctx.editor.dirty());
        ask(&mut p, &mut ctx);
        let invalid = TempAsset {
            name: file.name.clone(),
            file: std::env::temp_dir().join(&file.name),
        };
        answer_picker(&mut p, &mut ctx, request, Some(&invalid.file));
        assert!(p.error.as_deref().unwrap().contains("inside assets/"));
        assert!(!invalid.file.exists() && !ctx.quit_requested() && ctx.editor.dirty());
        ask(&mut p, &mut ctx);
        answer_picker(&mut p, &mut ctx, request, Some(&file.file));
        assert!(file.file.exists() && ctx.quit_requested() && !ctx.editor.dirty());
    }

    #[test]
    fn the_web_editor_saves_as_and_opens_through_the_path_field() {
        let file = TempAsset::new("web-path-field");
        let (egui, mut p, mut ctx) = web_editor();
        let e = the_entity(&mut ctx);
        ctx.edit(|ed, ctx| ed.rename(ctx, e, "Typed")).unwrap();

        // Untitled: Ctrl+S asks for the kind and the file in one dialog.
        frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
        let Some(Dialog::SaveAs { path, .. }) = p.dialog() else {
            panic!("Save as asks for the path: {:?}", p.dialog());
        };
        assert_eq!(path, super::super::DEFAULT_SCENE, "the field suggests the default name");
        answer_path_field(&egui, &mut p, &mut ctx, Some(&file.name));
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(p.dialog().is_none() && file.file.is_file() && !ctx.editor.dirty());
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));

        // Open: the field starts at the document's file; Esc keeps it.
        frame(&egui, &mut p, &mut ctx, vec![command(Key::N)]);
        assert_eq!(ctx.world.len(), 0);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        assert_eq!(p.dialog(), Some(&Dialog::Open { path: String::new() }));
        answer_path_field(&egui, &mut p, &mut ctx, None);
        assert!(p.dialog().is_none() && ctx.editor.doc.path.is_none() && ctx.world.is_empty());

        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        answer_path_field(&egui, &mut p, &mut ctx, Some(&file.name));
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
        let e = the_entity(&mut ctx);
        let name = ctx.world.get::<crate::hierarchy::EntityName>(e).map(|n| n.0.clone());
        assert_eq!(name.as_deref(), Some("Typed"), "the saved world came back");
    }

    #[test]
    fn the_web_editor_reports_a_path_field_error_and_keeps_the_dialog() {
        let (egui, mut p, mut ctx) = web_editor();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
        answer_path_field(&egui, &mut p, &mut ctx, Some("no-such-scene.json"));
        assert!(p.error.is_some(), "a missing file is reported");
        assert!(ctx.editor.doc.path.is_none());
    }

    #[test]
    fn keys_held_when_the_focus_left_are_released_when_it_returns() {
        let (egui, mut p, mut ctx) = editor(None);
        // Ctrl (⌘) and Enter go down; their key-up events go to another window.
        let down = vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
            key(Key::Enter, egui::Modifiers::COMMAND),
        ];
        raw_frame(&egui, &mut p, &mut ctx, down);
        raw_frame(&egui, &mut p, &mut ctx, vec![]);
        assert!(held(&egui), "nothing let them go yet");

        // The window loses focus and gets it back. On macOS egui-winit
        // reports both as focused (it asks the window late), so egui itself
        // keeps the keys: the panels forget them.
        p.focus_changed(false);
        p.focus_changed(true);
        let reported = vec![egui::Event::WindowFocused(true), egui::Event::WindowFocused(true)];
        raw_frame(&egui, &mut p, &mut ctx, reported);
        assert!(!held(&egui));

        // A key pressed once focus is back counts as usual.
        let again = vec![
            egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
            key(Key::Enter, egui::Modifiers::SHIFT),
        ];
        raw_frame(&egui, &mut p, &mut ctx, again);
        assert!(egui.input(|i| i.modifiers.shift && i.key_down(Key::Enter)));
    }

    #[test]
    fn the_system_dialog_leaves_no_modifier_or_enter_held() {
        let file = TempAsset::new("dialog-keys");
        let (egui, mut p, mut ctx) = editor(None);
        // Ctrl+S, then Enter on "Choose file…": neither key-up reaches the
        // editor while the dialog is up.
        let save = vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
            key(Key::S, egui::Modifiers::COMMAND),
        ];
        raw_frame(&egui, &mut p, &mut ctx, save);
        assert!(matches!(p.dialog(), Some(Dialog::SaveAs { .. })));
        raw_frame(&egui, &mut p, &mut ctx, vec![key(Key::Enter, egui::Modifiers::COMMAND)]);
        assert!(held(&egui));
        answer_picker(
            &mut p,
            &mut ctx,
            FilePicker::SaveAs {
                kind: DocKind::Scene,
                then_quit: false,
            },
            Some(&file.file),
        );
        // No focus event at all: the dialog's return is enough.
        raw_frame(&egui, &mut p, &mut ctx, vec![]);
        assert!(!held(&egui), "Ctrl and Enter are let go");
        assert!(file.file.is_file() && p.dialog().is_none());

        // A plain S is not Ctrl+S: the document isn't saved again.
        let e = the_entity(&mut ctx);
        ctx.edit(|ed, ctx| ed.rename(ctx, e, "After")).unwrap();
        raw_frame(&egui, &mut p, &mut ctx, vec![key(Key::S, egui::Modifiers::NONE)]);
        assert!(ctx.editor.dirty());
    }

    #[test]
    fn a_snapped_2d_gizmo_drag_in_the_viewport_is_one_undo_step() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        let start = ctx.world.get::<T2>(e).unwrap().position;
        p.select(Some(e), false);
        p.scene.gizmo2.snap.on = true;
        frame(&egui, &mut p, &mut ctx, vec![]);
        // Grab the x arrow halfway along, drag 37 pixels right (zoom 1).
        let center = ctx.camera.world_to_screen(start);
        let grab = center + Vec2::new(crate::edit2d::GIZMO_PIXELS * 0.5, 0.0);
        frame(&egui, &mut p, &mut ctx, vec![moved(grab)]);
        assert_eq!(p.scene.gizmo2.hover, Some(crate::edit2d::Handle::Axis(0)));
        frame(&egui, &mut p, &mut ctx, vec![button(grab, true)]);
        let to = grab + Vec2::new(37.0, 12.0);
        frame(&egui, &mut p, &mut ctx, vec![moved(to)]);
        frame(&egui, &mut p, &mut ctx, vec![button(to, false)]);
        let end = ctx.world.get::<T2>(e).unwrap().position;
        let snapped = crate::edit2d::snap(start.x + 37.0 / ctx.camera.zoom, 8.0);
        assert_eq!(end, Vec2::new(snapped, start.y), "along x only, on the grid");
        assert!(ctx.editor.dirty());
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert_eq!(ctx.world.get::<T2>(e).unwrap().position, start, "one step");
        assert!(!ctx.editor.dirty());

        // E: the rotate ring; a quarter turn of the mouse around it.
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::E)]);
        assert_eq!(p.scene.gizmo2.mode, crate::edit2d::Mode::Rotate);
        let r = crate::edit2d::GIZMO_PIXELS;
        let on_ring = center + Vec2::new(r, 0.0);
        frame(&egui, &mut p, &mut ctx, vec![moved(on_ring)]);
        frame(&egui, &mut p, &mut ctx, vec![button(on_ring, true)]);
        let quarter = center + Vec2::new(0.0, r);
        frame(&egui, &mut p, &mut ctx, vec![moved(quarter)]);
        frame(&egui, &mut p, &mut ctx, vec![button(quarter, false)]);
        let turned = ctx.world.get::<T2>(e).unwrap().rotation;
        assert!((turned - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{turned}");
    }

    #[test]
    fn a_file_changed_on_disk_with_nothing_unsaved_is_opened_again() {
        let file = TempAsset::new("watch");
        let (egui, mut p, mut ctx) = editor(None);
        ctx.edit(|ed, ctx| ed.save_as(ctx, &file.name, DocKind::Scene)).unwrap();
        let mut other = ctx.world.save();
        other["entities"][0]["components"]["Hp"] = serde_json::json!(6.0);
        std::fs::write(&file.file, serde_json::to_string(&other).unwrap() + "\n\n\n").unwrap();
        // The check runs every half second: wait it out in frames (60 checks'
        // worth: a hang detector, not a number of checks a busy machine makes).
        let until = web_time::Instant::now() + crate::editor::document_check_every() * 60;
        while ctx.world.query::<&Hp>().iter().next().map(|h| h.0) != Some(6.0) {
            assert!(web_time::Instant::now() < until, "not reloaded");
            frame(&egui, &mut p, &mut ctx, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            p.status.as_deref().is_some_and(|s| s.contains("changed on disk")),
            "{:?}",
            p.status
        );
    }

    #[test]
    fn dragging_a_hierarchy_row_onto_another_attaches_the_whole_selection_as_one_undo_step() {
        use crate::hierarchy::EntityName;
        let (egui, mut p, mut ctx) = editor(None);
        let middle = ctx.camera.center();
        let spawn = |ctx: &mut Ctx, name: &str, x: f32| {
            ctx.world
                .spawn((T2::at(middle + Vec2::new(x, 0.0)), EntityName::new(name)))
        };
        let (alpha, beta, gamma) = (
            spawn(&mut ctx, "Alpha", -80.0),
            spawn(&mut ctx, "Beta", 0.0),
            spawn(&mut ctx, "Gamma", 80.0),
        );
        p.select(Some(alpha), false);
        p.select(Some(beta), true);
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        let row = |name: &str| text_at(&shapes, |t| t.starts_with(name) && t.contains('#')).unwrap();
        let (from, onto) = (row("Alpha"), row("Gamma"));
        assert!(from.x < SIDE_PANEL, "the rows are in the hierarchy on the left: {from}");

        mouse_drag(&egui, &mut p, &mut ctx, from, onto, 4);
        let parent_of = |ctx: &Ctx, e| {
            crate::editor::hierarchy(&ctx.world)
                .into_iter()
                .find(|n| n.entity == e)
                .and_then(|n| n.parent.map(|(p, _)| p))
        };
        assert_eq!(parent_of(&ctx, alpha), Some(gamma), "{:?}", p.error);
        assert_eq!(parent_of(&ctx, beta), Some(gamma), "the whole selection moved");
        assert_eq!(parent_of(&ctx, gamma), None);
        let at = |ctx: &Ctx, e| ctx.world.get::<T2>(e).unwrap().position;
        assert_eq!(at(&ctx, alpha), middle + Vec2::new(-80.0, 0.0), "it stays where it is");

        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert_eq!(
            (parent_of(&ctx, alpha), parent_of(&ctx, beta)),
            (None, None),
            "one undo"
        );
        assert!(!ctx.editor.can_undo() && !ctx.editor.dirty());
    }

    #[test]
    fn dragging_a_number_in_the_inspector_is_one_undo_step() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        ctx.world.get_mut::<Hp>(e).unwrap().0 = 7.25;
        p.select(Some(e), false);
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        let field = text_at(&shapes, |t| t == "7.25").expect("the Hp field");
        assert!(
            field.x > SCREEN.x - SIDE_PANEL - 50.0,
            "in the inspector on the right: {field}"
        );

        mouse_drag(&egui, &mut p, &mut ctx, field, field + Vec2::new(60.0, 0.0), 6);
        let hp = ctx.world.get::<Hp>(e).unwrap().0;
        assert!(hp > 7.5, "dragging right raised it: {hp}");
        assert!(ctx.editor.dirty());
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert_eq!(
            ctx.world.get::<Hp>(e).unwrap().0,
            7.25,
            "one undo takes the whole drag back"
        );
        assert!(!ctx.editor.can_undo(), "it was one step");
        assert!(!ctx.editor.dirty());
    }

    #[cfg(feature = "3d")]
    mod three {
        use super::*;
        use crate::d3::Transform as T3;
        use crate::edit3d::{Mode, Ray, Space, axes, closest_on_axis, layout, world_to_screen};
        use glam::{Quat, Vec3, vec3};

        /// A 3D game in the editor: one entity at the origin, the camera at
        /// (0, 6, 10) looking at it. One frame has run.
        fn editor3d() -> (egui::Context, Panels, Ctx, Entity) {
            let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SCREEN.x, SCREEN.y));
            ctx.world.reflect::<Hp>();
            ctx.scene3d.camera.position = vec3(0.0, 6.0, 10.0);
            ctx.scene3d.camera.look_at(Vec3::ZERO);
            let e = ctx.world.spawn((T3::default(), Hp(1.0)));
            let mut p = Panels {
                app: Some(App::new(None)),
                ..Default::default()
            };
            let egui = egui::Context::default();
            frame(&egui, &mut p, &mut ctx, vec![]);
            (egui, p, ctx, e)
        }

        fn screen(ctx: &Ctx, p: Vec3) -> Vec2 {
            world_to_screen(&ctx.scene3d.camera, p).unwrap()
        }

        /// The point `t` of the way from the gizmo's middle to axis `i`'s tip.
        fn on_axis(ctx: &Ctx, mode: Mode, i: usize, t: f32) -> Vec2 {
            let l = layout(
                &ctx.scene3d.camera,
                mode,
                Vec3::ZERO,
                axes(Space::World, Quat::IDENTITY),
            )
            .unwrap();
            l.center.lerp(l.tips[i].unwrap(), t)
        }

        /// How far along the x axis the mouse at `at` points.
        fn along_x(ctx: &Ctx, at: Vec2) -> f32 {
            closest_on_axis(&Ray::from_screen(&ctx.scene3d.camera, at), Vec3::ZERO, Vec3::X).unwrap()
        }

        fn the_transform(ctx: &mut Ctx) -> T3 {
            *ctx.world.query::<(&T3, &Hp)>().iter().map(|(t, _)| t).next().unwrap()
        }

        /// Clicks the entity in the viewport and turns snapping on.
        fn select_it(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, e: Entity) {
            let at = screen(ctx, Vec3::ZERO);
            assert!(
                at.x > SIDE_PANEL && at.x < SCREEN.x - SIDE_PANEL,
                "in the viewport: {at}"
            );
            frame(egui, p, ctx, vec![moved(at), button(at, true)]);
            frame(egui, p, ctx, vec![button(at, false)]);
            assert_eq!(p.selected, Some(e), "a click in the viewport selects it");
            assert!(p.scene.active3d, "a 3D world opens in the 3D view");
            p.scene.gizmo.snap.on = true;
        }

        #[test]
        fn in_a_3d_game_snapped_move_rotate_and_scale_drags_are_one_undo_step_each() {
            let (egui, mut p, mut ctx, e) = editor3d();
            select_it(&egui, &mut p, &mut ctx, e);

            // Move: the x arrow, 2.2 units along; the 0.5 grid makes it 2.
            let grab = on_axis(&ctx, Mode::Translate, 0, 0.7);
            let to = screen(&ctx, vec3(along_x(&ctx, grab) + 2.2, 0.0, 0.0));
            mouse_drag(&egui, &mut p, &mut ctx, grab, to, 5);
            assert_eq!(p.selected, Some(e), "a gizmo drag keeps the selection");
            let moved = the_transform(&mut ctx).position;
            assert!((moved - vec3(2.0, 0.0, 0.0)).length() < 1e-4, "{moved}");
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert_eq!(the_transform(&mut ctx).position, Vec3::ZERO);
            assert!(!ctx.editor.can_undo() && !ctx.editor.dirty(), "one step");

            // Rotate (E): along the ground ring from 45° to 135°: a quarter
            // turn about y, on the 15° steps.
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::E)]);
            assert_eq!(p.scene.gizmo.mode, Mode::Rotate);
            let len = crate::edit3d::GIZMO_PIXELS * crate::edit3d::pixel_size(&ctx.scene3d.camera, Vec3::ZERO);
            let ring = |deg: f32| {
                let t = deg.to_radians();
                screen(&ctx, (Vec3::Z * t.cos() + Vec3::X * t.sin()) * len)
            };
            let (from, to) = (ring(45.0), ring(135.0));
            mouse_drag(&egui, &mut p, &mut ctx, from, to, 6);
            let turned = the_transform(&mut ctx).rotation;
            let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
            assert!(turned.angle_between(quarter) < 1e-3, "{turned:?}");
            assert!(ctx.editor.dirty());
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert!(the_transform(&mut ctx).rotation.angle_between(Quat::IDENTITY) < 1e-5);
            assert!(!ctx.editor.can_undo() && !ctx.editor.dirty(), "one step");

            // Scale (R): the x box, pulled to twice as far out.
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::R)]);
            let grab = on_axis(&ctx, Mode::Scale, 0, 0.7);
            let to = screen(&ctx, vec3(along_x(&ctx, grab) * 2.03, 0.0, 0.0));
            mouse_drag(&egui, &mut p, &mut ctx, grab, to, 5);
            let scaled = the_transform(&mut ctx).scale;
            assert!(
                (scaled - vec3(2.0, 1.0, 1.0)).length() < 1e-4,
                "{scaled}: snapped to 0.1"
            );
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert_eq!(the_transform(&mut ctx).scale, Vec3::ONE);
            assert!(!ctx.editor.can_undo() && !ctx.editor.dirty(), "one step");
        }

        #[test]
        fn the_3d_camera_centres_on_the_view_between_the_panels() {
            let (egui, mut p, mut ctx, e) = editor3d();
            let view = p
                .app
                .as_ref()
                .unwrap()
                .view
                .unwrap()
                .pixels(Vec2::new(SCREEN.x, SCREEN.y));
            assert!(view.min.x >= SIDE_PANEL - 1.0 && view.min.y > 10.0, "{view:?}");
            assert_eq!(
                ctx.scene3d.camera.screen_rect(),
                view,
                "the camera draws into the free part"
            );
            let aspect = ctx.scene3d.camera.viewport().x / ctx.scene3d.camera.viewport().y;
            assert!(
                (aspect - view.size().x / view.size().y).abs() < 1e-4,
                "its aspect is the view's"
            );
            let looked_at = screen(&ctx, Vec3::ZERO);
            assert!(
                (looked_at - view.center()).length() < 0.5,
                "what the camera looks at is in the middle of the view: {looked_at} vs {}",
                view.center()
            );
            // A click there picks it (`select_it` clicks where it is drawn).
            select_it(&egui, &mut p, &mut ctx, e);
        }

        #[test]
        fn an_image_dropped_on_a_model_paints_it_in_one_undo_step_and_the_scene_keeps_it() {
            let grid = TempAsset::copy_of("paint-grid", "grid.glb");
            let tile = TempAsset::copy_of("paint-tile", "tile.png");
            let scene = TempAsset::new("paint-scene");
            let (egui, mut p, mut ctx, e) = editor3d();
            // Meshes and textures load as in a headless run (no GPU here).
            ctx.set_headless(true);
            let mesh = ctx.gfx.load_mesh(&grid.name);
            ctx.world.insert(e, crate::render::Model::new(mesh));
            select_it(&egui, &mut p, &mut ctx, e);
            p.scene.forget_assets();
            search(&mut p, "ponko-test-app-paint-tile");
            let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
            let short = super::super::super::assets_panel::short_name(&tile.name);
            let from = text_at(&shapes, |t| t == short).expect("the image's tile");
            let on_model = screen(&ctx, Vec3::ZERO);
            let n = ctx.world.len();
            mouse_drag(&egui, &mut p, &mut ctx, from, on_model, 6);
            assert!(p.error.is_none(), "{:?}", p.error);
            assert_eq!(ctx.world.len(), n, "nothing new is placed");
            let painted = serde_json::json!({ "texture": tile.name });
            assert_eq!(ctx.world.get_json(e, "ModelMaterial").unwrap(), painted);
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert!(ctx.world.get_json(e, "ModelMaterial").is_err(), "one undo takes it off");
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Y)]);
            assert_eq!(ctx.world.get_json(e, "ModelMaterial").unwrap(), painted);

            // Dropped beside the model: nothing to paint.
            let beside = Vec2::new(SIDE_PANEL + 20.0, 120.0);
            let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
            let from = text_at(&shapes, |t| t == short).expect("the image's tile");
            mouse_drag(&egui, &mut p, &mut ctx, from, beside, 6);
            assert!(p.error.as_deref().unwrap_or("").contains("on a model"), "{:?}", p.error);

            ctx.edit(|ed, ctx| ed.save_as(ctx, &scene.name, DocKind::Scene))
                .unwrap();
            let saved = std::fs::read_to_string(&scene.file).unwrap();
            assert!(saved.contains(&format!("\"texture\": \"{}\"", tile.name)), "{saved}");
            p.open_file(&mut ctx, &scene.name);
            frame(&egui, &mut p, &mut ctx, vec![]);
            let e = the_entity(&mut ctx);
            assert_eq!(
                ctx.world.get_json(e, "ModelMaterial").unwrap(),
                painted,
                "opened again, still painted"
            );
        }

        #[test]
        fn a_gltf_scene_keeps_a_node_moved_in_the_view_through_a_reexport_and_a_reopen() {
            let yard = |hp: f32, wall_x: f32| {
                crate::three::cube_gltf(&serde_json::json!([
                    { "name": "Crate", "mesh": 0, "extras": { "ponko:Hp": hp } },
                    { "name": "Wall", "mesh": 0, "translation": [wall_x, 0.0, -3.0] },
                ]))
            };
            let name = format!("ponko-test-app-yard-{}.gltf", std::process::id());
            let file = TempAsset {
                file: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(&name),
                name,
            };
            std::fs::write(&file.file, yard(3.0, -3.0)).unwrap();
            let scene = TempAsset::new("yard-scene");
            let (egui, mut p, mut ctx, e) = editor3d();
            ctx.set_headless(true);
            ctx.world.despawn(e);
            // The game's setup spawns its level.
            let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
            let node = |ctx: &Ctx, root: Entity, path: &str| {
                ctx.world
                    .get::<crate::d3::GltfRoot>(root)
                    .unwrap()
                    .entity(path)
                    .unwrap()
            };
            let at = |ctx: &Ctx, e: Entity| ctx.world.get::<T3>(e).unwrap().position;
            let (crate_e, wall) = (node(&ctx, root, "Crate"), node(&ctx, root, "Wall"));
            select_it(&egui, &mut p, &mut ctx, crate_e);
            let grab = on_axis(&ctx, Mode::Translate, 0, 0.7);
            let to = screen(&ctx, vec3(along_x(&ctx, grab) + 2.2, 0.0, 0.0));
            mouse_drag(&egui, &mut p, &mut ctx, grab, to, 5);
            assert!(
                (at(&ctx, crate_e) - vec3(2.0, 0.0, 0.0)).length() < 1e-4,
                "moved by the gizmo"
            );
            ctx.edit(|ed, ctx| ed.set(ctx, crate_e, "Hp", serde_json::json!(8.0)))
                .unwrap();

            // Exported again: the crate keeps its place and Hp, the wall
            // goes where the file puts it now.
            std::fs::write(&file.file, yard(4.0, -5.0)).unwrap();
            ctx.reload_gltf(&file.name).unwrap();
            frame(&egui, &mut p, &mut ctx, vec![]);
            assert_eq!(node(&ctx, root, "Crate"), crate_e);
            assert!(
                (at(&ctx, crate_e) - vec3(2.0, 0.0, 0.0)).length() < 1e-4,
                "{}",
                at(&ctx, crate_e)
            );
            assert_eq!(ctx.world.get_json(crate_e, "Hp").unwrap(), serde_json::json!(8.0));
            assert!(
                (at(&ctx, wall) - vec3(-5.0, 0.0, -3.0)).length() < 1e-4,
                "{}",
                at(&ctx, wall)
            );

            // Saved and opened again: the scene is spawned again, with them.
            ctx.edit(|ed, ctx| ed.save_as(ctx, &scene.name, DocKind::Scene))
                .unwrap();
            p.open_file(&mut ctx, &scene.name);
            assert!(p.error.is_none(), "{:?}", p.error);
            frame(&egui, &mut p, &mut ctx, vec![]);
            let roots: Vec<Entity> = ctx
                .world
                .query::<(Entity, &crate::d3::GltfRoot)>()
                .iter()
                .map(|(e, _)| e)
                .collect();
            assert_eq!(roots.len(), 1, "the scene once");
            let crate_e = node(&ctx, roots[0], "Crate");
            assert!(
                (at(&ctx, crate_e) - vec3(2.0, 0.0, 0.0)).length() < 1e-4,
                "{}",
                at(&ctx, crate_e)
            );
            assert_eq!(ctx.world.get_json(crate_e, "Hp").unwrap(), serde_json::json!(8.0));
            // And a click where it was moved to picks it.
            let there = screen(&ctx, vec3(2.0, 0.0, 0.0));
            frame(&egui, &mut p, &mut ctx, vec![moved(there), button(there, true)]);
            frame(&egui, &mut p, &mut ctx, vec![button(there, false)]);
            assert_eq!(p.selected, Some(crate_e));
        }

        #[test]
        fn a_3d_scene_saved_from_the_editor_opens_again_as_it_was_saved() {
            let file = TempAsset::new("scene-3d");
            let (egui, mut p, mut ctx, e) = editor3d();
            select_it(&egui, &mut p, &mut ctx, e);
            let grab = on_axis(&ctx, Mode::Translate, 0, 0.7);
            let to = screen(&ctx, vec3(along_x(&ctx, grab) + 3.0, 0.0, 0.0));
            mouse_drag(&egui, &mut p, &mut ctx, grab, to, 4);
            assert!((the_transform(&mut ctx).position - vec3(3.0, 0.0, 0.0)).length() < 1e-4);

            // Ctrl+S on an untitled scene uses the scene kind and the chosen file.
            frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
            answer_picker(
                &mut p,
                &mut ctx,
                FilePicker::SaveAs {
                    kind: DocKind::Scene,
                    then_quit: false,
                },
                Some(&file.file),
            );
            assert!(p.dialog().is_none() && !ctx.editor.dirty(), "{:?}", p.error);
            assert!(std::fs::read_to_string(&file.file).unwrap().contains("Transform3D"));

            // Change it, then open the file again over the change.
            let grab = on_axis_at(&ctx, vec3(3.0, 0.0, 0.0));
            let to = screen(&ctx, vec3(3.0 + along_x_at(&ctx, grab, 3.0) + 2.0, 0.0, 0.0));
            mouse_drag(&egui, &mut p, &mut ctx, grab, to, 4);
            assert!(ctx.editor.dirty());
            assert!((the_transform(&mut ctx).position - vec3(5.0, 0.0, 0.0)).length() < 1e-4);
            frame(&egui, &mut p, &mut ctx, vec![command(Key::O)]);
            assert_eq!(p.dialog(), Some(&Dialog::PickOpen));
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
            answer_picker(&mut p, &mut ctx, FilePicker::Open, Some(&file.file));
            assert!(p.dialog().is_none(), "{:?} {:?}", p.dialog(), p.error);
            assert!(
                (the_transform(&mut ctx).position - vec3(3.0, 0.0, 0.0)).length() < 1e-4,
                "as saved"
            );
            assert_eq!(ctx.world.len(), 1);
            assert!(!ctx.editor.dirty() && !ctx.editor.can_undo(), "a fresh history");
            assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
            frame(&egui, &mut p, &mut ctx, vec![]);
            assert!(p.scene.active3d && p.selected.is_none());
        }

        /// A `.gltf` asset of these nodes (see `cube_gltf`), named for this
        /// test run.
        fn gltf_asset(what: &str, nodes: &serde_json::Value) -> TempAsset {
            let name = format!("ponko-test-app-{what}-{}.gltf", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&name);
            std::fs::write(&file, crate::three::cube_gltf(nodes)).unwrap();
            TempAsset { name, file }
        }

        /// A crate at the origin with Hp 3, a wall, and a lamp on the wall.
        fn yard_nodes(wall_x: f32) -> serde_json::Value {
            serde_json::json!([
                { "name": "Crate", "mesh": 0, "extras": { "ponko:Hp": 3.0 } },
                { "name": "Wall", "mesh": 0, "translation": [wall_x, 0.0, -3.0], "children": [2] },
                { "name": "Lamp", "mesh": 0, "translation": [0.0, 2.0, 0.0] },
            ])
        }

        /// The editor with the game's level, `file`, spawned by its setup.
        fn editor_with_level(file: &TempAsset) -> (egui::Context, Panels, Ctx, Entity) {
            let (egui, mut p, mut ctx, e) = editor3d();
            ctx.set_headless(true);
            ctx.world.despawn(e);
            let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
            frame(&egui, &mut p, &mut ctx, vec![]);
            (egui, p, ctx, root)
        }

        /// The entity of node `path` of the only glTF scene.
        fn gltf_node(ctx: &mut Ctx, path: &str) -> Entity {
            let roots: Vec<Entity> = ctx
                .world
                .query::<(Entity, &crate::d3::GltfRoot)>()
                .iter()
                .map(|(e, _)| e)
                .collect();
            assert_eq!(roots.len(), 1, "one scene");
            ctx.world
                .get::<crate::d3::GltfRoot>(roots[0])
                .unwrap()
                .entity(path)
                .unwrap()
        }

        fn parent_of(ctx: &Ctx, e: Entity) -> Option<Entity> {
            ctx.world.get::<crate::attach::Attached3D>(e).map(|a| a.to)
        }

        fn position(ctx: &Ctx, e: Entity) -> Vec3 {
            ctx.world.get::<T3>(e).unwrap().position
        }

        /// Drags the selection's x arrow (the gizmo at `from`) 2.2 units
        /// along with snapping on: to `from + 2` on x.
        fn drag_x(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, from: Vec3) {
            let grab = on_axis_at(ctx, from);
            let to = screen(ctx, vec3(from.x + along_x_at(ctx, grab, from.x) + 2.2, 0.0, 0.0));
            mouse_drag(egui, p, ctx, grab, to, 5);
        }

        #[test]
        fn the_gltf_panel_lists_a_scene_nodes_overrides_and_each_revert_puts_one_back() {
            let file = gltf_asset("gltf-panel", &yard_nodes(-3.0));
            let (egui, mut p, mut ctx, _) = editor_with_level(&file);
            let crate_e = gltf_node(&mut ctx, "Crate");
            select_it(&egui, &mut p, &mut ctx, crate_e);
            let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
            assert!(
                text_at(&shapes, |t| t == "Node Crate of").is_some(),
                "the scene's section"
            );
            assert!(text_at(&shapes, |t| t == "Same as the file").is_some());

            // Moved with the gizmo, its Hp changed, the wall deleted with
            // the Delete key.
            drag_x(&egui, &mut p, &mut ctx, Vec3::ZERO);
            assert!((position(&ctx, crate_e) - vec3(2.0, 0.0, 0.0)).length() < 1e-4);
            ctx.edit(|ed, ctx| ed.set(ctx, crate_e, "Hp", serde_json::json!(8.0)))
                .unwrap();
            let wall = gltf_node(&mut ctx, "Wall");
            p.select(Some(wall), false);
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::Delete)]);
            assert!(!ctx.world.is_alive(wall), "deleted");
            p.select(Some(crate_e), false);
            let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
            let all = texts(&shapes);
            for row in ["Crate › Attached3D", "Crate › Hp", "Wall", "Wall/Lamp"] {
                assert!(all.iter().any(|t| t == row), "{row} is listed: {all:?}");
            }
            assert!(all.iter().any(|t| t == "deleted"));
            assert!(all.iter().any(|t| t == "changed: offset"), "{all:?}");

            // The first Revert puts the crate where the file has it.
            click_text(&egui, &mut p, &mut ctx, |t| t == "Revert");
            assert!(p.error.is_none(), "{:?}", p.error);
            assert!(position(&ctx, crate_e).length() < 1e-4, "{}", position(&ctx, crate_e));
            assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(8.0)), "only that one");
            click_text(&egui, &mut p, &mut ctx, |t| t == "Revert");
            assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(3.0)));
            // The deleted wall's Revert spawns it again, with its lamp.
            click_text(&egui, &mut p, &mut ctx, |t| t == "Revert");
            assert!(p.error.is_none(), "{:?}", p.error);
            let (wall, lamp) = (gltf_node(&mut ctx, "Wall"), gltf_node(&mut ctx, "Wall/Lamp"));
            assert!(ctx.world.is_alive(wall) && ctx.world.is_alive(lamp));
            assert!(ctx.world.get::<crate::render::Model>(wall).is_some());
            assert_eq!(parent_of(&ctx, lamp), Some(wall));
            let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
            assert!(
                text_at(&shapes, |t| t == "Same as the file").is_some(),
                "{:?}",
                texts(&shapes)
            );

            // Each Revert is an undo step.
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert!(!ctx.world.is_alive(wall), "the wall is deleted again");
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(8.0)));
        }

        #[test]
        fn gltf_nodes_deleted_or_attached_elsewhere_in_the_editor_stay_so_through_a_reexport_and_a_reopen() {
            let file = gltf_asset("gltf-tree", &yard_nodes(-3.0));
            let scene = TempAsset::new("gltf-tree-scene");
            let (egui, mut p, mut ctx, _) = editor_with_level(&file);
            let (crate_e, wall, lamp) = (
                gltf_node(&mut ctx, "Crate"),
                gltf_node(&mut ctx, "Wall"),
                gltf_node(&mut ctx, "Wall/Lamp"),
            );
            // The lamp's row dragged onto the crate's in the hierarchy.
            let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
            let row = |name: &str| text_at(&shapes, |t| t.starts_with(name) && t.contains('#')).unwrap();
            let (from, onto) = (row("Lamp"), row("Crate"));
            assert!(from.x < SIDE_PANEL, "in the hierarchy: {from}");
            mouse_drag(&egui, &mut p, &mut ctx, from, onto, 4);
            assert_eq!(parent_of(&ctx, lamp), Some(crate_e), "{:?}", p.error);
            let lamp_at = position(&ctx, lamp);
            // The wall clicked in the view, then Delete.
            let there = screen(&ctx, vec3(-3.0, 0.0, -3.0));
            assert!(there.x > SIDE_PANEL, "in the view: {there}");
            frame(&egui, &mut p, &mut ctx, vec![moved(there), button(there, true)]);
            frame(&egui, &mut p, &mut ctx, vec![button(there, false)]);
            assert_eq!(p.selected, Some(wall));
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::Delete)]);
            assert!(!ctx.world.is_alive(wall));
            assert!(ctx.world.is_alive(lamp), "the lamp hangs off the crate now");

            // Exported again with the wall elsewhere: still deleted, the
            // lamp still on the crate where it was put.
            std::fs::write(&file.file, crate::three::cube_gltf(&yard_nodes(-5.0))).unwrap();
            ctx.reload_gltf(&file.name).unwrap();
            frame(&egui, &mut p, &mut ctx, vec![]);
            let wall = gltf_node(&mut ctx, "Wall");
            assert!(!ctx.world.is_alive(wall), "not spawned again");
            assert_eq!(gltf_node(&mut ctx, "Wall/Lamp"), lamp);
            assert_eq!(parent_of(&ctx, lamp), Some(crate_e));
            assert!(
                (position(&ctx, lamp) - lamp_at).length() < 1e-4,
                "{}",
                position(&ctx, lamp)
            );

            // Saved and opened again.
            ctx.edit(|ed, ctx| ed.save_as(ctx, &scene.name, DocKind::Scene))
                .unwrap();
            p.open_file(&mut ctx, &scene.name);
            assert!(p.error.is_none(), "{:?}", p.error);
            frame(&egui, &mut p, &mut ctx, vec![]);
            let (crate_e, wall, lamp) = (
                gltf_node(&mut ctx, "Crate"),
                gltf_node(&mut ctx, "Wall"),
                gltf_node(&mut ctx, "Wall/Lamp"),
            );
            assert!(!ctx.world.is_alive(wall), "still deleted");
            assert_eq!(parent_of(&ctx, lamp), Some(crate_e), "still on the crate");
            assert!(
                (position(&ctx, lamp) - lamp_at).length() < 1e-4,
                "{}",
                position(&ctx, lamp)
            );
            let models = ctx.world.query::<&crate::render::Model>().iter().count();
            assert_eq!(models, 2, "the crate and the lamp, once");
        }

        #[test]
        fn play_then_stop_spawns_gltf_scenes_again_and_undo_still_reaches_their_nodes() {
            let file = gltf_asset("gltf-play", &yard_nodes(-3.0));
            let (egui, mut p, mut ctx, _) = editor_with_level(&file);
            let crate_e = gltf_node(&mut ctx, "Crate");
            select_it(&egui, &mut p, &mut ctx, crate_e);
            drag_x(&egui, &mut p, &mut ctx, Vec3::ZERO);
            ctx.edit(|ed, ctx| ed.set(ctx, crate_e, "Hp", serde_json::json!(8.0)))
                .unwrap();
            let wall = gltf_node(&mut ctx, "Wall");
            ctx.edit(|ed, ctx| ed.delete(ctx, wall)).unwrap();
            p.select(Some(crate_e), false);
            frame(&egui, &mut p, &mut ctx, vec![]);

            // Played: the game moves the crate. Stopped.
            frame(&egui, &mut p, &mut ctx, vec![command(Key::P)]);
            assert!(ctx.editor.playing());
            ctx.world.get_mut::<T3>(crate_e).unwrap().position.y += 5.0;
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Period)]);
            assert!(!ctx.editor.playing() && p.dialog().is_none(), "{:?}", p.error);

            // The scene is there again, as it was edited.
            let now = gltf_node(&mut ctx, "Crate");
            assert!(ctx.world.is_alive(now), "spawned again");
            assert!(
                (position(&ctx, now) - vec3(2.0, 0.0, 0.0)).length() < 1e-4,
                "{}",
                position(&ctx, now)
            );
            assert_eq!(ctx.world.get::<Hp>(now), Some(&Hp(8.0)));
            assert!(ctx.world.get::<crate::render::Model>(now).is_some());
            let wall = gltf_node(&mut ctx, "Wall");
            assert!(!ctx.world.is_alive(wall), "still deleted");
            assert_eq!(p.selected, Some(now), "the selection follows it");

            // Undo walks back through the edits made before Play.
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            let (wall, lamp) = (gltf_node(&mut ctx, "Wall"), gltf_node(&mut ctx, "Wall/Lamp"));
            assert!(
                ctx.world.is_alive(wall) && ctx.world.is_alive(lamp),
                "the delete undone"
            );
            assert!(ctx.world.get::<crate::render::Model>(wall).is_some());
            assert_eq!(parent_of(&ctx, lamp), Some(wall));
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert_eq!(ctx.world.get::<Hp>(now), Some(&Hp(3.0)));
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
            assert!(position(&ctx, now).length() < 1e-4, "{}", position(&ctx, now));
            assert!(!ctx.editor.can_undo() && !ctx.editor.dirty());
            frame(&egui, &mut p, &mut ctx, vec![command(Key::Y)]);
            assert!((position(&ctx, now) - vec3(2.0, 0.0, 0.0)).length() < 1e-4, "redo too");
        }

        /// The x arrow's grab point with the gizmo at `pivot`.
        fn on_axis_at(ctx: &Ctx, pivot: Vec3) -> Vec2 {
            let l = layout(
                &ctx.scene3d.camera,
                Mode::Translate,
                pivot,
                axes(Space::World, Quat::IDENTITY),
            )
            .unwrap();
            l.center.lerp(l.tips[0].unwrap(), 0.7)
        }

        /// How far past `x` along the x axis the mouse at `at` points.
        fn along_x_at(ctx: &Ctx, at: Vec2, x: f32) -> f32 {
            closest_on_axis(&Ray::from_screen(&ctx.scene3d.camera, at), vec3(x, 0.0, 0.0), Vec3::X).unwrap()
        }
    }

    /// [`editor`] that keeps its settings in `settings`, for project
    /// "/games/test".
    fn editor_with(settings: &std::path::Path) -> (egui::Context, Panels, Ctx) {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SCREEN.x, SCREEN.y));
        ctx.world.reflect::<Hp>();
        let middle = ctx.camera.center();
        ctx.world.spawn((T2::at(middle), Hp(1.0)));
        let mut panels = Panels {
            app: Some(App::with_settings(
                None,
                Some(settings.to_path_buf()),
                "/games/test".into(),
            )),
            ..Default::default()
        };
        let egui = egui::Context::default();
        frame(&egui, &mut panels, &mut ctx, vec![]);
        (egui, panels, ctx)
    }

    #[test]
    fn a_rebound_shortcut_replaces_the_old_key_and_is_remembered_next_time() {
        use super::super::keys::{Action, parse};
        use super::super::prefs::ShortcutsWindow;
        let file = super::super::settings::tests::TempSettings::new("rebind");
        let (egui, mut p, mut ctx) = editor_with(&file.0);
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::F1)]);
        assert!(
            p.app.as_ref().unwrap().prefs.shortcuts.is_some(),
            "F1 opens the shortcuts"
        );
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(text_at(&shapes, |t| t == "Duplicate the selection").is_some());
        assert!(text_at(&shapes, |t| t == "Ctrl+D").is_some(), "each binding is listed");

        // "Set" on Duplicate, then the new key.
        p.app.as_mut().unwrap().prefs.shortcuts = Some(ShortcutsWindow {
            capturing: Some(Action::Duplicate),
        });
        let e = the_entity(&mut ctx);
        p.select(Some(e), false);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::J)]);
        assert_eq!(p.keys.keys(Action::Duplicate), [parse("Ctrl+J").unwrap()]);
        assert_eq!(ctx.world.len(), 1, "the key that was captured did nothing else");
        p.toggle_shortcuts();

        frame(&egui, &mut p, &mut ctx, vec![command(Key::D)]);
        assert_eq!(ctx.world.len(), 1, "the old key does nothing now");
        frame(&egui, &mut p, &mut ctx, vec![command(Key::J)]);
        assert_eq!(ctx.world.len(), 2, "the new key duplicates");

        let (egui, mut p, mut ctx) = editor_with(&file.0);
        assert_eq!(p.keys.keys(Action::Duplicate), [parse("Ctrl+J").unwrap()]);
        let e = the_entity(&mut ctx);
        p.select(Some(e), false);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::J)]);
        assert_eq!(ctx.world.len(), 2, "the next run has the rebound key");
    }

    #[test]
    fn esc_while_setting_a_shortcut_keeps_the_old_one() {
        use super::super::keys::Action;
        use super::super::prefs::ShortcutsWindow;
        let file = super::super::settings::tests::TempSettings::new("esc");
        let (egui, mut p, mut ctx) = editor_with(&file.0);
        p.app.as_mut().unwrap().prefs.shortcuts = Some(ShortcutsWindow {
            capturing: Some(Action::Save),
        });
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Escape)]);
        assert_eq!(p.keys.keys(Action::Save), Action::Save.defaults());
        assert_eq!(
            p.app.as_ref().unwrap().prefs.shortcuts,
            Some(ShortcutsWindow { capturing: None })
        );
    }

    #[test]
    fn panel_sizes_and_recent_files_are_restored_next_time() {
        let file = super::super::settings::tests::TempSettings::new("layout");
        let scene = TempAsset::new("recent");
        {
            let (egui, mut p, mut ctx) = editor_with(&file.0);
            assert_eq!(p.layout(), super::super::settings::Layout::default());
            p.set_dialog(Some(Dialog::SaveAs {
                path: scene.name.clone(),
                kind: DocKind::Scene,
                then_quit: false,
            }));
            frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
            answer_picker(
                &mut p,
                &mut ctx,
                FilePicker::SaveAs {
                    kind: DocKind::Scene,
                    then_quit: false,
                },
                Some(&scene.file),
            );
            frame(&egui, &mut p, &mut ctx, vec![]);
            assert!(p.error.is_none(), "{:?}", p.error);
            assert_eq!(p.recent_files(), std::slice::from_ref(&scene.name));
            // Drag the hierarchy's edge 120 points to the right.
            let edge = Vec2::new(p.layout().left + 1.0, 400.0);
            mouse_drag(&egui, &mut p, &mut ctx, edge, edge + Vec2::new(120.0, 0.0), 4);
            frame(&egui, &mut p, &mut ctx, vec![]);
            assert!((p.layout().left - 420.0).abs() < 4.0, "{:?}", p.layout());
        }
        let (egui, mut p, mut ctx) = editor_with(&file.0);
        assert!((p.layout().left - 420.0).abs() < 4.0, "{:?}", p.layout());
        assert_eq!(p.recent_files(), std::slice::from_ref(&scene.name));
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        let heading = text_at(&shapes, |t| t == "Hierarchy").unwrap();
        let components = text_at(&shapes, |t| t.starts_with("Nothing selected")).unwrap();
        assert!(components.x > 420.0, "the right panel starts after the wider hierarchy");
        assert!(heading.x < 420.0);
    }

    #[test]
    fn play_pause_and_stop_keys_run_the_game_and_stop_puts_the_world_back_exactly() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        let before = ctx.world.save();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::P)]);
        assert!(ctx.editor.playing() && !ctx.time.is_paused(), "Ctrl+P plays");
        // The game runs: it moves the entity and spawns another.
        ctx.world.get_mut::<T2>(e).unwrap().position.x += 50.0;
        ctx.world.spawn((T2::default(),));
        let key = |k, m| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: m,
        };
        frame(
            &egui,
            &mut p,
            &mut ctx,
            vec![key(Key::P, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)],
        );
        assert!(ctx.time.is_paused() && ctx.editor.playing(), "Ctrl+Shift+P pauses");
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::F10)]);
        assert!(
            ctx.time.is_paused() && ctx.editor.playing(),
            "F10 steps and stays paused"
        );
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Period)]);
        assert!(p.dialog().is_none(), "nothing edited while playing: no question");
        assert!(!ctx.editor.playing());
        assert_eq!(ctx.world.save(), before, "Stop restores the scene exactly");
        assert!(!ctx.editor.dirty());
    }

    #[test]
    fn stop_asks_about_edits_made_while_playing_and_keeps_the_ticked_ones() {
        use crate::editor::play::PlayEdit;
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        frame(&egui, &mut p, &mut ctx, vec![command(Key::P)]);
        ctx.edit(|ed, ctx| ed.set(ctx, e, "Hp", serde_json::json!(5.0)))
            .unwrap();
        ctx.world.get_mut::<T2>(e).unwrap().position.x += 50.0;
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Period)]);
        let edit = PlayEdit::Component(e, "Hp".into());
        assert_eq!(
            p.dialog(),
            Some(&Dialog::Stop {
                keep: vec![(edit, true)]
            })
        );
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(
            text_at(&shapes, |t| t.ends_with(": Hp")).is_some(),
            "the edit is listed"
        );
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        assert!(p.dialog().is_none() && !ctx.editor.playing());
        assert_eq!(ctx.world.get::<Hp>(e), Some(&Hp(5.0)), "kept");
        let middle = ctx.camera.center();
        assert_eq!(
            ctx.world.get::<T2>(e).unwrap().position,
            middle,
            "the game's move is undone"
        );
        assert!(ctx.editor.dirty(), "a kept edit is an unsaved change");
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert_eq!(ctx.world.get::<Hp>(e), Some(&Hp(1.0)), "and one undo step");
    }

    #[test]
    fn stop_keeping_none_of_the_edits_restores_the_scene_exactly() {
        let (egui, mut p, mut ctx) = editor(None);
        let e = the_entity(&mut ctx);
        let before = ctx.world.save();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::P)]);
        ctx.edit(|ed, ctx| ed.set(ctx, e, "Hp", serde_json::json!(5.0)))
            .unwrap();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Period)]);
        if let Some(Dialog::Stop { keep }) = &mut p.app.as_mut().unwrap().dialog {
            keep[0].1 = false;
        }
        frame(&egui, &mut p, &mut ctx, vec![plain(Key::Enter)]);
        assert_eq!(ctx.world.save(), before);
        assert!(!ctx.editor.dirty());
    }

    /// Clicks the first text `wanted` accepts (a button's label).
    fn click_text(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx, wanted: impl Fn(&str) -> bool) {
        let shapes = drawn(egui, p, ctx, vec![]);
        let at = text_at(&shapes, wanted).expect("the text to click is drawn");
        frame(egui, p, ctx, vec![moved(at)]);
        frame(egui, p, ctx, vec![button(at, true)]);
        frame(egui, p, ctx, vec![button(at, false)]);
    }

    /// A prefab of an entity with Hp 1.0, placed in the editor and selected.
    fn placed_prefab(file: &TempAsset) -> (egui::Context, Panels, Ctx, Entity) {
        let prefab = serde_json::json!({
            "format": "ponko-prefab", "version": 2, "entities": [
                { "entity": 1, "components": {
                    "Transform2D": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                    "Hp": 1.0 } },
                { "entity": 2, "components": {
                    "Transform2D": { "position": [10.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                    "Attached": { "to": 1, "offset": [10.0, 0.0], "rotation": 0.0 },
                    "Hp": 2.0 } }
            ]
        });
        std::fs::write(&file.file, prefab.to_string()).unwrap();
        let (egui, mut p, mut ctx) = editor(None);
        let root = ctx
            .edit(|ed, ctx| ed.spawn_prefab(ctx, &file.name, Vec2::new(400.0, 300.0)))
            .unwrap();
        p.select(Some(root), false);
        frame(&egui, &mut p, &mut ctx, vec![]);
        (egui, p, ctx, root)
    }

    #[test]
    fn the_prefab_panel_lists_an_override_and_its_revert_button_puts_it_back() {
        let file = TempAsset::new("panel-revert");
        let (egui, mut p, mut ctx, root) = placed_prefab(&file);
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(text_at(&shapes, |t| t == "Same as the prefab").is_some());

        ctx.edit(|ed, ctx| ed.set(ctx, root, "Hp", serde_json::json!(7.0)))
            .unwrap();
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(text_at(&shapes, |t| t == "Hp").is_some(), "the override is listed");
        click_text(&egui, &mut p, &mut ctx, |t| t == "Revert");
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(ctx.world.get::<Hp>(root), Some(&Hp(1.0)));
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(text_at(&shapes, |t| t == "Same as the prefab").is_some());
    }

    #[test]
    fn apply_to_prefab_from_the_panel_writes_the_file_and_updates_the_other_instance() {
        let file = TempAsset::new("panel-apply");
        let (egui, mut p, mut ctx, root) = placed_prefab(&file);
        let other = ctx
            .edit(|ed, ctx| ed.spawn_prefab(ctx, &file.name, Vec2::new(100.0, 100.0)))
            .unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, root, "Hp", serde_json::json!(9.0)))
            .unwrap();
        click_text(&egui, &mut p, &mut ctx, |t| t == "Apply to prefab");
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(
            p.status.as_deref().unwrap_or("").contains("1 other instance"),
            "{:?}",
            p.status
        );
        assert_eq!(ctx.world.get::<Hp>(other), Some(&Hp(9.0)));
        let text = std::fs::read_to_string(&file.file).unwrap();
        assert!(text.contains("9.0"), "{text}");
    }

    #[test]
    fn a_prefab_document_applies_its_saved_changes_to_the_scenes() {
        let file = TempAsset::new("doc-prefab");
        let scene = TempAsset::new("doc-scene");
        let (egui, mut p, mut ctx, _) = placed_prefab(&file);
        ctx.edit(|ed, ctx| ed.save_as(ctx, &scene.name, DocKind::Scene))
            .unwrap();
        p.open_file(&mut ctx, &file.name);
        frame(&egui, &mut p, &mut ctx, vec![]);
        assert_eq!(ctx.editor.doc.kind, DocKind::Prefab);
        let root = the_entity_with_hp(&mut ctx, 1.0);
        ctx.edit(|ed, ctx| ed.set(ctx, root, "Hp", serde_json::json!(4.0)))
            .unwrap();
        frame(&egui, &mut p, &mut ctx, vec![command(Key::S)]);
        assert!(!ctx.editor.dirty());
        click_text(&egui, &mut p, &mut ctx, |t| t == "Apply to scenes");
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(
            p.status.as_deref().unwrap_or("").contains(&scene.name),
            "{:?}",
            p.status
        );
        let saved = std::fs::read_to_string(&scene.file).unwrap();
        assert!(saved.contains("4.0"), "the scene's instance has the new Hp: {saved}");
        assert_eq!(ctx.editor.doc.kind, DocKind::Prefab, "the prefab is open again");
    }

    #[test]
    fn apply_buttons_write_an_instance_and_the_prefab_it_includes_back_to_their_files() {
        let inner = TempAsset::new("nested-inner");
        let outer = TempAsset::new("nested-outer");
        let cart = serde_json::json!({
            "format": "ponko-prefab", "version": 2, "entities": [
                { "entity": 1, "components": {
                    "Transform2D": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] }, "Hp": 1.0 } },
                { "entity": 2, "components": {
                    "Transform2D": { "position": [10.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                    "Attached": { "to": 1, "offset": [10.0, 0.0], "rotation": 0.0 }, "Hp": 2.0 } }
            ]
        });
        std::fs::write(&inner.file, cart.to_string()).unwrap();
        let train = serde_json::json!({
            "format": "ponko-prefab", "version": 2, "entities": [
                { "entity": 1, "components": {
                    "Transform2D": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] }, "Hp": 3.0 } },
                { "entity": 2, "prefab": inner.name, "components": {
                    "Transform2D": { "position": [40.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] } } }
            ]
        });
        std::fs::write(&outer.file, train.to_string()).unwrap();
        let (egui, mut p, mut ctx) = editor(None);
        let a = ctx
            .edit(|ed, ctx| ed.spawn_prefab(ctx, &outer.name, Vec2::new(400.0, 300.0)))
            .unwrap();
        let b = ctx
            .edit(|ed, ctx| ed.spawn_prefab(ctx, &outer.name, Vec2::new(500.0, 400.0)))
            .unwrap();
        let parts = |ctx: &Ctx, r| ctx.world.get::<crate::scene::PrefabParts>(r).unwrap().0.clone();
        // The included cart's wheel (Hp 2) of a, selected and changed.
        let wheel = parts(&ctx, a)[1];
        p.select(Some(wheel), false);
        ctx.edit(|ed, ctx| ed.set(ctx, wheel, "Hp", serde_json::json!(8.0)))
            .unwrap();
        click_text(&egui, &mut p, &mut ctx, |t| t == format!("Apply to {}", inner.name));
        assert!(p.error.is_none(), "{:?}", p.error);
        let cart_now: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&inner.file).unwrap()).unwrap();
        assert_eq!(cart_now["entities"][1]["components"]["Hp"], 8.0, "{cart_now:#}");
        assert_eq!(
            ctx.world.get::<Hp>(parts(&ctx, b)[1]),
            Some(&Hp(8.0)),
            "b's cart follows"
        );

        // Then the included cart's root, applied to the train: a link with
        // the change.
        let cart_a = parts(&ctx, a)[0];
        p.select(Some(cart_a), false);
        ctx.edit(|ed, ctx| ed.set(ctx, cart_a, "Hp", serde_json::json!(5.0)))
            .unwrap();
        click_text(&egui, &mut p, &mut ctx, |t| t == "Apply to prefab");
        assert!(p.error.is_none(), "{:?}", p.error);
        let train_now: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&outer.file).unwrap()).unwrap();
        assert_eq!(train_now["entities"][1]["prefab"], inner.name.as_str(), "{train_now:#}");
        assert_eq!(train_now["entities"][1]["components"]["Hp"], 5.0);
        assert_eq!(
            ctx.world.get::<Hp>(parts(&ctx, b)[0]),
            Some(&Hp(5.0)),
            "b follows the train"
        );
    }

    #[test]
    fn the_2d_camera_centres_on_the_view_between_the_panels_and_follows_a_resized_panel() {
        let (egui, mut p, mut ctx) = editor(None);
        // Spawned in the middle of the window before the editor opened.
        let e = the_entity(&mut ctx);
        let at = |ctx: &Ctx| ctx.camera.world_to_screen(ctx.world.get::<T2>(e).unwrap().position);
        let view = |p: &Panels| {
            let v = p.app.as_ref().unwrap().view.unwrap();
            v.pixels(Vec2::new(SCREEN.x, SCREEN.y))
        };
        let before = view(&p);
        assert!(
            before.min.x >= SIDE_PANEL - 1.0 && before.max.x <= SCREEN.x - SIDE_PANEL + 1.0,
            "between the side panels: {before:?}"
        );
        assert!(
            before.min.y > 10.0 && before.max.y < SCREEN.y - 10.0,
            "below the menu, above the assets: {before:?}"
        );
        assert_eq!(ctx.camera.screen_rect(), before, "the camera draws into the free part");
        assert!(
            (at(&ctx) - before.center()).length() < 0.5,
            "what was in the middle of the window is in the middle of the view: {} vs {}",
            at(&ctx),
            before.center()
        );
        let middle = before.center();
        frame(&egui, &mut p, &mut ctx, vec![moved(middle), button(middle, true)]);
        frame(&egui, &mut p, &mut ctx, vec![button(middle, false)]);
        assert_eq!(p.selected, Some(e), "a click in the middle of the view picks it");

        // Drag the hierarchy's edge 100 px to the right.
        let edge = Vec2::new(before.min.x, before.center().y);
        mouse_drag(&egui, &mut p, &mut ctx, edge, edge + Vec2::new(100.0, 0.0), 5);
        let after = view(&p);
        assert!(
            (after.min.x - before.min.x - 100.0).abs() < 2.0,
            "{before:?} -> {after:?}"
        );
        assert!(
            (at(&ctx) - after.center()).length() < 0.5,
            "still in the middle of the narrower view: {} vs {}",
            at(&ctx),
            after.center()
        );
    }

    fn the_entity_with_hp(ctx: &mut Ctx, hp: f32) -> Entity {
        let mut q = ctx.world.query::<(Entity, &Hp)>();
        q.iter().find(|(_, h)| h.0 == hp).map(|(e, _)| e).unwrap()
    }

    /// Every text drawn, for failure messages.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    /// Runs frames until the asset panel's thumbnails are made.
    fn with_thumbnails(egui: &egui::Context, p: &mut Panels, ctx: &mut Ctx) -> Vec<egui::epaint::ClippedShape> {
        frame(egui, p, ctx, vec![]);
        assert!(p.app.as_mut().unwrap().assets.wait_for_thumbnails());
        frame(egui, p, ctx, vec![]);
        drawn(egui, p, ctx, vec![])
    }

    /// Searches the asset panel for `text`.
    fn search(p: &mut Panels, text: &str) {
        p.app.as_mut().unwrap().assets.search = text.to_string();
    }

    #[test]
    fn the_asset_panel_shows_a_tile_per_asset_and_search_narrows_it() {
        let tile = TempAsset::copy_of("tiles-png", "tile.png");
        let beep = TempAsset::copy_of("tiles-wav", "beep.wav");
        let (egui, mut p, mut ctx) = editor(None);
        p.scene.forget_assets();
        search(&mut p, "ponko-test-app-tiles");
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        let short = |name: &str| super::super::assets_panel::short_name(name);
        assert!(
            text_at(&shapes, |t| t == short(&tile.name)).is_some(),
            "the texture's tile"
        );
        assert!(
            text_at(&shapes, |t| t == short(&beep.name)).is_some(),
            "the sound's tile"
        );
        assert!(text_at(&shapes, |t| t == "2 of " || t.starts_with("2 of ")).is_some());

        search(&mut p, "TILES .wav");
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(
            text_at(&shapes, |t| t.starts_with("1 of ")).is_some(),
            "search is by every word, any case: {:?}",
            texts(&shapes)
        );
        let thumbs = egui.tex_manager().read().num_allocated();
        assert!(thumbs >= 2, "the thumbnails are textures: {thumbs}");
    }

    #[test]
    fn a_texture_dragged_from_the_asset_panel_into_the_view_becomes_a_sprite_there_in_one_undo_step() {
        let tile = TempAsset::copy_of("drag-tile", "tile.png");
        let (egui, mut p, mut ctx) = editor(None);
        // Textures load as in a headless run (no GPU here).
        ctx.set_headless(true);
        p.scene.forget_assets();
        search(&mut p, "ponko-test-app-drag-tile");
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        let short = super::super::assets_panel::short_name(&tile.name);
        let from = text_at(&shapes, |t| t == short).expect("the tile");
        let to = Vec2::new(SCREEN.x / 2.0, 200.0);
        let before = ctx.world.len();
        // Where the view shows it now: the new sprite's components widen
        // the inspector, and the view keeps its middle when it narrows.
        let dropped_at = ctx.camera.screen_to_world(to);
        mouse_drag(&egui, &mut p, &mut ctx, from, to, 6);
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(ctx.world.len(), before + 1);
        let e = p.selected.expect("the new sprite is selected");
        let sprite = ctx.world.get_json(e, "Sprite").unwrap();
        assert_eq!(sprite["texture"], tile.name.as_str());
        let at = ctx.world.get::<T2>(e).unwrap().position;
        assert!((at - dropped_at).length() < 1.0, "placed where it was dropped: {at}");
        frame(&egui, &mut p, &mut ctx, vec![command(Key::Z)]);
        assert_eq!(ctx.world.len(), before, "one undo step takes it away");
    }

    /// A model thumbnail kept on disk under the model's content (as the
    /// renderer left it in an earlier run) is the tile's picture at once.
    #[cfg(feature = "3d")]
    #[test]
    fn a_model_tile_shows_the_thumbnail_kept_on_disk_for_its_content() {
        let grid = TempAsset::copy_of("cached-grid", "grid.glb");
        let cache = std::env::temp_dir().join(format!("ponko-app-thumb-cache-{}", std::process::id()));
        let bytes = std::fs::read(&grid.file).unwrap();
        let key = crate::editor::thumbs::content_key(&grid.file, &bytes);
        std::fs::create_dir_all(&cache).unwrap();
        let size = crate::editor::thumbs::SIZE as u32;
        image::RgbaImage::from_pixel(size, size, image::Rgba([255, 0, 255, 255]))
            .save(cache.join(format!("{key}.png")))
            .unwrap();
        let (egui, mut p, mut ctx) = editor(None);
        p.app.as_mut().unwrap().assets.use_thumbnail_cache(cache.clone());
        search(&mut p, "ponko-test-app-cached-grid");
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        let short = super::super::assets_panel::short_name(&grid.name);
        assert!(text_at(&shapes, |t| t == short).is_some(), "the tile");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let thumb = p
            .app
            .as_mut()
            .unwrap()
            .assets
            .thumbnails()
            .get(&root, &grid.name, crate::editor::cook::AssetKind::Model)
            .cloned();
        let _ = std::fs::remove_dir_all(&cache);
        let thumb = thumb.expect("made").unwrap();
        assert_eq!(
            &thumb.rgba[..4],
            [255, 0, 255, 255],
            "the kept picture, not drawn again"
        );
        // Clicked, its details give what the file holds.
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        let at = text_at(&shapes, |t| t == short).expect("the tile");
        frame(&egui, &mut p, &mut ctx, vec![moved(at)]);
        frame(&egui, &mut p, &mut ctx, vec![button(at, true)]);
        frame(&egui, &mut p, &mut ctx, vec![button(at, false)]);
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(
            text_at(&shapes, |t| t.starts_with("model, ") && t.ends_with(" triangles")).is_some(),
            "its triangle count: {:?}",
            texts(&shapes)
        );
    }

    /// The sound plays through the offline mixer only: nothing is heard.
    #[cfg(feature = "audio")]
    #[test]
    fn a_selected_sound_previews_through_the_mixer_and_stops() {
        let beep = TempAsset::copy_of("preview-beep", "beep.wav");
        let (egui, mut p, mut ctx) = editor(None);
        ctx.audio.open_offline(22_050);
        p.scene.forget_assets();
        search(&mut p, "ponko-test-app-preview-beep");
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        let short = super::super::assets_panel::short_name(&beep.name);
        let at = text_at(&shapes, |t| t == short).expect("the tile");
        frame(&egui, &mut p, &mut ctx, vec![moved(at)]);
        frame(&egui, &mut p, &mut ctx, vec![button(at, true)]);
        frame(&egui, &mut p, &mut ctx, vec![button(at, false)]);
        assert_eq!(
            p.app.as_ref().unwrap().assets.selected.as_deref(),
            Some(beep.name.as_str())
        );
        let shapes = drawn(&egui, &mut p, &mut ctx, vec![]);
        assert!(
            text_at(&shapes, |t| t.starts_with("sound, ") && t.ends_with(" s")).is_some(),
            "its length"
        );
        let silent = ctx.audio.render_offline(0.05);
        assert!(silent.iter().all(|s| s[0] == 0.0 && s[1] == 0.0));

        click_text(&egui, &mut p, &mut ctx, |t| t == "Preview");
        assert!(p.app.as_ref().unwrap().assets.previewing.is_some());
        let mix = ctx.audio.render_offline(0.05);
        let peak = mix.iter().fold(0.0f32, |m, s| m.max(s[0].abs()));
        assert!(peak > 0.01, "the preview is in the mix: {peak}");

        click_text(&egui, &mut p, &mut ctx, |t| t == "Stop");
        assert!(p.app.as_ref().unwrap().assets.previewing.is_none());
        let _ = ctx.audio.render_offline(0.05); // the stop's short fade
        let after = ctx.audio.render_offline(0.05);
        assert!(after.iter().all(|s| s[0].abs() < 1e-4), "stopped");
    }

    #[test]
    fn import_settings_show_where_they_come_from_and_reset_removes_the_assets_own_rule() {
        use crate::editor::cook::{CookSettings, Source};
        let tile = TempAsset::copy_of("cook-tile", "tile.png");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let cook_file = TempAsset {
            name: "cook.json".into(),
            file: root.join("cook.json"),
        };
        assert!(
            !cook_file.file.exists(),
            "the engine's assets have no cook.json of their own"
        );
        let pattern = "ponko-test-app-cook-tile-*.png".to_string();
        std::fs::write(
            &cook_file.file,
            serde_json::json!({ "assets": { pattern.clone(): { "texture": "mask" } } }).to_string(),
        )
        .unwrap();
        let (egui, mut p, mut ctx) = editor(None);
        p.scene.forget_assets();
        search(&mut p, "ponko-test-app-cook-tile");
        p.app.as_mut().unwrap().assets.selected = Some(tile.name.clone());
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        assert!(
            text_at(&shapes, |t| t == format!("From the pattern \"{pattern}\"")).is_some(),
            "{:?}",
            texts(&shapes)
        );
        assert!(
            text_at(&shapes, |t| t == "mask").is_some(),
            "the pattern's texture kind"
        );

        // A change writes the asset's own rule (what the combo does).
        let mut cook = CookSettings::load(&root).unwrap();
        super::super::assets_panel::set_rule(&mut cook, &tile.name, Some(serde_json::json!({ "texture": "linear" })))
            .unwrap();
        cook.save().unwrap();
        p.app.as_mut().unwrap().assets = Default::default();
        p.app.as_mut().unwrap().assets.search = "ponko-test-app-cook-tile".into();
        p.app.as_mut().unwrap().assets.selected = Some(tile.name.clone());
        let shapes = with_thumbnails(&egui, &mut p, &mut ctx);
        assert!(text_at(&shapes, |t| t == "Its own settings").is_some());
        assert!(text_at(&shapes, |t| t == "linear").is_some());

        click_text(&egui, &mut p, &mut ctx, |t| t == "Use the pattern / defaults");
        let cook = CookSettings::load(&root).unwrap();
        assert_eq!(cook.rule_for(&tile.name).1, Source::Pattern(pattern));
    }
}
