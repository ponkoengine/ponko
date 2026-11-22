//! The runtime inspector: an egui overlay, toggled with the
//! backquote key (`` ` ``, left of 1), that shows the
//! running game. Sim controls (pause, step, speed), frame and system times,
//! every entity with its components, live editing of reflected components
//! and resources, and picking units in the world.
//!
//! Paused, it's an editor ([`crate::editor`]): select, drag, duplicate and
//! delete entities in the world, add and remove components, undo and redo,
//! place prefabs, save and load scenes, and Play / Stop. A hierarchy panel
//! parents entities by drag and drop, and an asset browser drags meshes,
//! prefabs and materials into the world. In 3D, a click picks along the
//! mouse ray and a gizmo moves, turns and scales the selection.
//!
//! Built with the `inspector` feature; turned on per game with
//! [`Game::inspector`](crate::game::Game::inspector) (on in debug builds). While
//! closed, it costs nothing: egui doesn't run and no events are kept.

use std::collections::HashSet;

use egui::{Button, CollapsingHeader, Color32, DragValue, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, vec2};
use glam::Vec2;
use ponko_ecs::{ComponentId, Entity, World};
use serde_json::Value;

use crate::d2::Sprite;
use crate::d2::Transform as Transform2D;
use crate::reflect::short_name;
use crate::{camera::Rect, ctx::Ctx, reflect::WorldReflect};

#[cfg(feature = "editor")]
#[path = "inspector_app.rs"]
pub(crate) mod app;
#[cfg(feature = "editor")]
#[path = "inspector_assets.rs"]
mod assets_panel;
#[cfg(feature = "editor")]
#[path = "inspector_author.rs"]
pub(crate) mod author;
#[cfg(all(test, feature = "editor"))]
#[path = "inspector_author_harness.rs"]
pub(crate) mod author_harness;
#[path = "inspector_debug.rs"]
mod debug_panel;
#[cfg(feature = "editor")]
#[path = "inspector_file_picker.rs"]
mod file_picker;
#[cfg(all(feature = "editor", feature = "3d"))]
#[path = "inspector_gltf.rs"]
mod gltf_panel;
#[cfg(feature = "editor")]
#[path = "inspector_graph.rs"]
pub(crate) mod graph_panel;
#[path = "inspector_keys.rs"]
pub(crate) mod keys;
#[cfg(all(feature = "editor", feature = "3d"))]
#[path = "inspector_material.rs"]
pub(crate) mod material_panel;
#[cfg(feature = "editor")]
#[path = "inspector_particles.rs"]
pub(crate) mod particles_panel;
#[cfg(feature = "editor")]
#[path = "inspector_play.rs"]
mod play;
#[cfg(feature = "editor")]
#[path = "inspector_prefab.rs"]
mod prefab_panel;
#[cfg(feature = "editor")]
#[path = "inspector_prefs.rs"]
mod prefs;
#[path = "inspector_scene.rs"]
mod scene;
#[cfg(feature = "editor")]
#[path = "inspector_settings.rs"]
pub(crate) mod settings;
#[cfg(all(feature = "editor", feature = "3d"))]
#[path = "inspector_terrain.rs"]
pub(crate) mod terrain_panel;
#[cfg(feature = "editor")]
#[path = "inspector_tilemap.rs"]
pub(crate) mod tilemap_panel;

/// Environment variable that turns the inspector on (`1`) or off (`0`),
/// overriding [`Game::inspector`](crate::game::Game::inspector).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const ENV_INSPECTOR: &str = "PONKO_INSPECTOR";

/// The key that opens and closes the inspector.
pub(crate) const TOGGLE_CODE: winit::keyboard::KeyCode = winit::keyboard::KeyCode::Backquote;

/// Whether the inspector is on, and whether it starts open:
/// `PONKO_INSPECTOR` (`1`, `0` or `open`) if set, else the game's choice.
/// In web builds, the page's `?inspector` (`=1`, `=0`, `=open`) instead.
pub(crate) fn enabled(game_default: bool) -> (bool, bool) {
    #[cfg(target_arch = "wasm32")]
    let setting = crate::web::query_param("inspector").map(|v| if v.is_empty() { "1".into() } else { v });
    #[cfg(not(target_arch = "wasm32"))]
    let setting = std::env::var(ENV_INSPECTOR).ok();
    match setting.as_deref() {
        Some("1") => (true, false),
        Some("open") => (true, true),
        Some("0") => (false, false),
        _ => (game_default, false),
    }
}

/// Frames kept for the frame-time graph.
const HISTORY: usize = 240;
/// How close (window pixels) a click must be to an entity to pick it.
const PICK_RADIUS: f32 = 24.0;
/// Height of one row in the entity list.
const ROW_HEIGHT: f32 = 18.0;
/// Where Ctrl+D puts the copy, from the original (world units).
const DUPLICATE_OFFSET: Vec2 = Vec2::new(16.0, 16.0);
/// Folder of `assets/` the prefab palette lists.
const PREFAB_FOLDER: &str = "prefabs";
/// Scene file used when none is typed.
const DEFAULT_SCENE: &str = "levels/level.json";

/// What the overlay draws this frame: egui's triangles and texture changes.
pub(crate) struct Overlay {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

impl Overlay {
    /// Adds a frame that wasn't drawn yet: its texture changes still apply.
    pub fn merge_older(&mut self, mut older: Overlay) {
        let mut textures = std::mem::take(&mut older.textures);
        textures.append(std::mem::take(&mut self.textures));
        self.textures = textures;
    }
}

impl Drop for Overlay {
    /// A frame that's never drawn (the game quits first) is fine to drop:
    /// its textures die with the renderer.
    fn drop(&mut self) {
        self.textures.clear();
    }
}

/// The inspector's panels and what's selected in them. Independent of the
/// window, so it can be run (and tested) on any egui context.
#[derive(Default)]
pub(crate) struct Panels {
    pub selected: Option<Entity>,
    /// The rest of a multi-selection (Shift-click), without `selected`.
    others: Vec<Entity>,
    /// Hierarchy, asset browser, and the 3D view's gizmo and camera.
    scene: scene::SceneEdit,
    /// Only list entities with this component.
    filter: Option<ComponentId>,
    /// The next click in the world selects the entity under it.
    pub picking: bool,
    /// A click to pick at (window pixels), from the window events.
    pub pick_at: Option<Vec2>,
    /// The last edit that failed, shown until the next one.
    error: Option<String>,
    frame_ms: Vec<f32>,
    frame_at: usize,
    /// Non-empty archetypes that pass the filter, and the first row of each.
    rows: Vec<(usize, usize)>,
    reflected: HashSet<String>,
    /// Paused with the panels open: clicks and shortcut keys in the world
    /// go to the editor, not the game.
    pub editing: bool,
    /// A prefab the next click in the world places.
    placing: Option<String>,
    /// Prefab files in `assets/prefabs/`; `None` until listed.
    prefabs: Option<Vec<String>>,
    scene_path: String,
    prefab_path: String,
    /// What the last scene or prefab action did.
    status: Option<String>,
    /// `Ctx::scene_loads` when the panels last ran: a load since then
    /// replaced the entities, so the selection may name another one.
    scene_loads: u64,
    /// The editor's shortcuts (rebindable in the editor app).
    pub keys: keys::Keymap,
    /// Edit mode: the panels are the editor's whole window (see
    /// `inspector_app.rs`), not an overlay on the game.
    #[cfg(feature = "editor")]
    pub app: Option<app::App>,
    /// The next frame starts with no key or modifier held (see
    /// [`Panels::input_lost`]).
    forget_held: bool,
}

impl Panels {
    /// The keyboard was elsewhere: the window's focus changed, or a system
    /// dialog had it. Keys let go meanwhile never send their key-up here, so
    /// the next frame forgets every key and modifier held (Ctrl from Ctrl+O,
    /// the Enter that confirmed a dialog); keys still down press again.
    pub(crate) fn input_lost(&mut self) {
        self.forget_held = true;
    }

    /// The window's focus changed (`focused`: it came back). Either way the
    /// held keys are forgotten: keys let go while it was away never send
    /// their key-up here, and on macOS egui-winit reads a late focus loss as
    /// focused (it asks the window), so egui would keep them held.
    pub(crate) fn focus_changed(&mut self, focused: bool) {
        let _ = focused;
        self.input_lost();
    }

    /// Runs one frame of the panels on `egui` with input `raw`: what
    /// [`Panels::input_lost`] asked to forget first, then `raw`'s events.
    pub fn run(&mut self, egui: &egui::Context, mut raw: egui::RawInput, ctx: &mut Ctx) -> egui::FullOutput {
        if std::mem::take(&mut self.forget_held) {
            // egui's own reset for a window that lost focus: no key down,
            // no modifier held.
            raw.events.insert(0, egui::Event::WindowFocused(false));
        }
        egui.run_ui(raw, |ui| self.show(ui, ctx))
    }

    /// Draws the inspector into `ui` and applies what was changed in it to `ctx`.
    pub fn show(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        #[cfg(feature = "editor")]
        if self.app.is_some() {
            self.show_app(ui, ctx);
            return;
        }
        self.begin_frame(ui, ctx);
        egui::Panel::right("ponko-inspector")
            .default_size(360.0)
            .resizable(true)
            .show(ui, |ui| {
                ScrollArea::vertical().id_salt("inspector-scroll").show(ui, |ui| {
                    self.sim_controls(ui, ctx);
                    self.frame_graph(ui);
                    if let Some(error) = &self.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    CollapsingHeader::new("Edit")
                        .default_open(true)
                        .show(ui, |ui| self.edit_tools(ui, ctx));
                    if let Some(e) = self.selected {
                        ui.separator();
                        self.entity(ui, ctx, e);
                        ui.separator();
                    }
                    CollapsingHeader::new("Hierarchy")
                        .default_open(ctx.world.len() <= scene::HIERARCHY_OPEN_UP_TO)
                        .show(ui, |ui| self.hierarchy_panel(ui, ctx));
                    CollapsingHeader::new("Assets")
                        .default_open(false)
                        .show(ui, |ui| self.asset_browser(ui, ctx));
                    CollapsingHeader::new("Entities")
                        .default_open(true)
                        .show(ui, |ui| self.entity_list(ui, ctx));
                    if !ctx.world.reflected_resources().is_empty() {
                        CollapsingHeader::new("Resources")
                            .default_open(true)
                            .show(ui, |ui| self.resources(ui, ctx));
                    }
                    CollapsingHeader::new("Systems")
                        .default_open(true)
                        .show(ui, |ui| systems(ui, ctx));
                    CollapsingHeader::new("GPU passes")
                        .default_open(false)
                        .show(ui, |ui| gpu_passes(ui, ctx));
                    let toggles = ctx.debug.toggles().next().is_some();
                    CollapsingHeader::new("Debug drawing")
                        .default_open(toggles)
                        .show(ui, |ui| debug_panel::panel(ui, ctx));
                });
            });
        self.world_and_keys(ui, ctx);
    }

    /// The mouse and keys in the world (while editing), and the selection
    /// and gizmo drawn over it.
    pub(crate) fn world_and_keys(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        if self.editing {
            if self.scene.active3d {
                self.world_input_3d(ui, ctx);
            } else {
                self.world_input(ui, ctx);
            }
            if !self.capturing_key() {
                self.shortcuts(ui, ctx);
            }
        } else {
            self.end_gizmo_drag2(ctx);
            self.end_gizmo_drag(ctx);
        }
        if self.scene.active3d {
            self.highlight_3d(ui, ctx);
        } else {
            self.highlight(ui, ctx);
        }
    }

    /// What every frame of the panels starts with: the frame time, the
    /// reflected names, and a selection that still names live entities of
    /// this world.
    pub(crate) fn begin_frame(&mut self, ui: &Ui, ctx: &mut Ctx) {
        self.editing = ctx.time.is_paused();
        self.record_frame(ctx.time.dt() * 1000.0);
        // A number dragged in the panels is one undo step until the button
        // goes up.
        if !ui.input(|i| i.pointer.any_down()) {
            ctx.editor.end_merge();
        }
        self.reflected.clear();
        self.reflected
            .extend(ctx.world.reflected_components().into_iter().map(str::to_owned));
        if self.scene_loads != ctx.scene_loads {
            self.scene_loads = ctx.scene_loads;
            self.select(None, false);
            self.scene.forget_drag();
            self.scene.renaming = None;
        }
        if self.selected.is_some_and(|e| !ctx.world.is_alive(e)) {
            self.selected = None;
        }
        self.scene_frame(ctx);
        if let Some(at) = self.pick_at.take() {
            self.picking = false;
            if let Some(e) = self.pick_any(ctx, at) {
                self.select(Some(e), false);
            }
        }
    }

    /// Whether the panels are the editor's window (edit mode).
    pub(crate) fn is_app(&self) -> bool {
        #[cfg(feature = "editor")]
        {
            self.app.is_some()
        }
        #[cfg(not(feature = "editor"))]
        {
            false
        }
    }

    /// The Shortcuts window is waiting for a key (never in the overlay).
    #[cfg(not(feature = "editor"))]
    fn capturing_key(&self) -> bool {
        false
    }

    /// Shows an edit's error, or forgets the last one.
    fn report<T, E: Into<String>>(&mut self, result: Result<T, E>) -> Option<T> {
        match result {
            Ok(v) => {
                self.error = None;
                Some(v)
            }
            Err(e) => {
                self.error = Some(e.into());
                None
            }
        }
    }

    /// Mouse in the world while paused: a gizmo handle drags, a click
    /// selects (or places the armed prefab) and dragging what was clicked
    /// moves the selection, right or middle drag pans, the wheel zooms.
    fn world_input(&mut self, ui: &Ui, ctx: &mut Ctx) {
        let (pos, pressed, down, pan, scroll, shift) = ui.input(|i| {
            let p = &i.pointer;
            let pan = if p.secondary_down() || p.middle_down() {
                p.delta()
            } else {
                egui::Vec2::ZERO
            };
            let pos = p.latest_pos().map(|p| Vec2::new(p.x, p.y));
            (
                pos,
                p.primary_pressed(),
                p.primary_down(),
                pan,
                i.smooth_scroll_delta.y,
                i.modifiers.shift,
            )
        });
        let Some(pos) = pos else { return };
        if self.drop_asset(ui, ctx, pos) {
            return;
        }
        let over_ui = ui.ctx().is_pointer_over_egui();
        if !over_ui {
            if pan != egui::Vec2::ZERO {
                ctx.camera.position -= Vec2::new(pan.x, pan.y) / ctx.camera.zoom;
            }
            if scroll != 0.0 {
                ctx.camera.zoom_at(pos, 1.1f32.powf(scroll / 50.0));
            }
        }
        // A brush of an authoring window (tiles) paints instead.
        #[cfg(feature = "editor")]
        if self.author_viewport(ui, ctx, pos) {
            return;
        }
        // A drag in progress follows the mouse until the button goes up.
        if self.scene.gizmo2.drag.is_some() {
            if down {
                self.update_gizmo_drag2(ctx, pos);
                return;
            }
            self.end_gizmo_drag2(ctx);
        }
        self.scene.gizmo2.hover = if over_ui { None } else { self.gizmo_hit2(ctx, pos) };
        if !pressed || over_ui {
            return;
        }
        if let Some(handle) = self.scene.gizmo2.hover {
            let mode = self.scene.gizmo2.mode;
            self.begin_gizmo_drag2(ctx, mode, handle, pos);
            return;
        }
        let add = ui.input(|i| i.modifiers.command);
        self.click(ctx, pos, shift, add);
    }

    /// A click in the world at window point `at`: places the armed prefab,
    /// or selects what's there and starts moving the selection with it.
    fn click(&mut self, ctx: &mut Ctx, at: Vec2, keep_placing: bool, add: bool) {
        let world_at = crate::d2::View::camera_at(&ctx.camera, &ctx.views2d, at).screen_to_world(at);
        if let Some(path) = self.placing.clone() {
            let world_at = if self.scene.gizmo2.snap.on {
                let step = self.scene.gizmo2.snap.step;
                world_at.map(|v| crate::edit2d::snap(v, step))
            } else {
                world_at
            };
            let placed = ctx.edit(|ed, ctx| ed.spawn_prefab(ctx, &path, world_at));
            if let Some(e) = self.report(placed) {
                self.select(Some(e), false);
            }
            if !keep_placing {
                self.placing = None;
            }
            return;
        }
        let picked = pick(ctx, at);
        // Ctrl-click (or Shift-click on an entity) adds to the selection.
        let add = add || (keep_placing && picked.is_some());
        self.select(picked, add);
        if add || picked.is_none() {
            return;
        }
        self.begin_gizmo_drag2(ctx, crate::edit2d::Mode::Translate, crate::edit2d::Handle::Center, at);
    }

    /// Keys while paused (by default): Delete, Ctrl+D (duplicate), Ctrl+Z
    /// (undo), Ctrl+Shift+Z or Ctrl+Y (redo), Esc; W, E, R, and in 3D F.
    /// See [`keys::Keymap`].
    fn shortcuts(&mut self, ui: &Ui, ctx: &mut Ctx) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        if self.selection_keys(ui, ctx) {
            return;
        }
        if self.scene.active3d {
            self.keys_3d(ui, ctx);
        }
        let (delete, duplicate, undo, escape) = ui.input(|i| {
            use keys::Action;
            (
                self.keys.pressed(i, Action::Delete),
                self.keys.pressed(i, Action::Duplicate),
                self.keys.pressed(i, Action::Undo),
                self.keys.pressed(i, Action::Cancel),
            )
        });
        if escape {
            self.placing = None;
        }
        if undo {
            self.undo(ctx);
        }
        if !self.scene.active3d {
            self.keys_2d(ui);
        }
        let Some(e) = self.selected else { return };
        if delete {
            let deleted = ctx.edit(|ed, ctx| ed.delete(ctx, e));
            self.report(deleted);
            self.select(None, false);
        } else if duplicate {
            let copy = ctx.edit(|ed, ctx| ed.duplicate(ctx, e, DUPLICATE_OFFSET));
            if let Some(copy) = self.report(copy) {
                self.select(Some(copy), false);
            }
        }
    }

    fn undo(&mut self, ctx: &mut Ctx) {
        let undone = ctx.edit(|ed, ctx| ed.undo(ctx));
        self.report(undone);
    }

    /// Undo, new entity, scenes, and the prefab palette.
    fn edit_tools(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        ui.horizontal(|ui| {
            if ui.add_enabled(ctx.editor.can_undo(), Button::new("Undo")).clicked() {
                self.undo(ctx);
            }
            if ui
                .button("New entity")
                .on_hover_text("An entity with only a transform, mid-screen")
                .clicked()
            {
                let at = ctx.camera.center();
                let e = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, at));
                self.select(Some(e), false);
            }
        });
        self.scene_tools(ui, ctx);
        if !self.editing {
            ui.label(RichText::new("Pause to edit in the world").weak());
        } else {
            ui.label(
                RichText::new(
                    "Click selects (Ctrl adds), drag moves, right-drag pans, wheel zooms. Del, Ctrl+D, Ctrl+Z.",
                )
                .weak(),
            );
        }

        ui.horizontal(|ui| {
            ui.label("Scene");
            // An empty field means the scene loaded last, else the default.
            let current = ctx.scene_file.clone().unwrap_or_else(|| DEFAULT_SCENE.to_owned());
            ui.add(
                TextEdit::singleline(&mut self.scene_path)
                    .hint_text(&current)
                    .desired_width(150.0),
            );
            let path = if self.scene_path.is_empty() {
                current
            } else {
                self.scene_path.clone()
            };
            if ui.button("Save").clicked() {
                self.save_scene(ctx, &path);
            }
            if ui
                .button("Load")
                .on_hover_text("Replaces the world; clears undo")
                .clicked()
            {
                self.open_file(ctx, &path);
            }
        });

        ui.horizontal(|ui| {
            ui.label("Prefab");
            let hint = format!("{PREFAB_FOLDER}/thing.json");
            ui.add(
                TextEdit::singleline(&mut self.prefab_path)
                    .hint_text(hint)
                    .desired_width(150.0),
            );
            let save = ui.add_enabled(
                self.selected.is_some() && !self.prefab_path.is_empty(),
                Button::new("Save"),
            );
            if let Some(e) = self.selected
                && save.on_hover_text("Saves the selected entity as this prefab").clicked()
            {
                let path = self.prefab_path.clone();
                let saved = ctx
                    .assets
                    .save_prefab(&ctx.world, e, &path)
                    .map_err(String::from)
                    .map(|()| format!("saved {path}"));
                self.status = self.report(saved);
                self.prefabs = None;
                self.scene.forget_assets();
            }
        });
        let prefabs = self
            .prefabs
            .get_or_insert_with(|| crate::assets::list(PREFAB_FOLDER, "json"))
            .clone();
        ui.horizontal_wrapped(|ui| {
            for path in &prefabs {
                let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".json");
                let armed = self.placing.as_deref() == Some(path.as_str());
                let tip = "Click, then click in the world to place it (hold Shift to place more)";
                if ui.selectable_label(armed, name).on_hover_text(tip).clicked() {
                    self.placing = if armed { None } else { Some(path.clone()) };
                }
            }
            if ui
                .small_button("⟳")
                .on_hover_text(format!("List assets/{PREFAB_FOLDER}/ again"))
                .clicked()
            {
                self.prefabs = None;
            }
        });
        if let Some(status) = &self.status {
            ui.label(RichText::new(status).weak());
        }
    }

    fn record_frame(&mut self, ms: f32) {
        if self.frame_ms.len() < HISTORY {
            self.frame_ms.push(ms);
        } else {
            self.frame_ms[self.frame_at] = ms;
        }
        self.frame_at = (self.frame_at + 1) % HISTORY;
    }

    fn sim_controls(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        ui.horizontal(|ui| {
            if ctx.time.is_paused() {
                let tip = if ctx.editor.playing() {
                    "Resume"
                } else {
                    "Run the game; Stop puts the world back as it is now"
                };
                if ui.button("Play").on_hover_text(tip).clicked() {
                    ctx.edit(|ed, ctx| ed.play(ctx));
                }
            } else if ui.button("Pause").clicked() {
                ctx.time.pause();
            }
            if ctx.editor.playing() {
                let stop = ui
                    .button("Stop")
                    .on_hover_text("Pause and put the world back as it was at Play");
                if stop.clicked() {
                    let stopped = ctx.edit(|ed, ctx| ed.stop(ctx));
                    self.report(stopped);
                    self.scene.forget_drag();
                }
                if ui
                    .button("Keep")
                    .on_hover_text("Keep the world as it is: Stop won't go back")
                    .clicked()
                {
                    ctx.edit(|ed, _| ed.keep());
                }
            }
            if ui.button("Step").on_hover_text("Run one tick (pauses)").clicked() {
                ctx.time.pause();
                ctx.time.step();
            }
            let mut speed = ctx.time.speed();
            let drag = DragValue::new(&mut speed).range(0.0..=16.0).speed(0.02).suffix("×");
            if ui.add(drag).on_hover_text("Simulation speed").changed() {
                ctx.time.set_speed(speed);
            }
        });
        ui.label(format!(
            "tick {}   {:.0} fps   {} entities",
            ctx.time.sim_tick(),
            ctx.time.fps(),
            ctx.world.len()
        ));
    }

    /// The last frames' times as bars; the line marks 60 fps.
    fn frame_graph(&self, ui: &mut Ui) {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, Color32::from_black_alpha(80));
        let target = 1000.0 / 60.0;
        // Scaled to the slowest frame, but at most 4× the target, so one
        // long frame (loading) doesn't flatten the rest.
        let top = self
            .frame_ms
            .iter()
            .copied()
            .fold(target * 2.0, f32::max)
            .min(target * 4.0);
        let bar = rect.width() / HISTORY as f32;
        let n = self.frame_ms.len();
        for i in 0..n {
            // Oldest on the left.
            let ms = if n < HISTORY {
                self.frame_ms[i]
            } else {
                self.frame_ms[(self.frame_at + i) % HISTORY]
            };
            let h = rect.height() * (ms / top).min(1.0);
            let x = rect.left() + i as f32 * bar;
            let color = if ms > target * 1.5 {
                Color32::from_rgb(230, 90, 70)
            } else {
                Color32::from_rgb(90, 180, 120)
            };
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + bar, rect.bottom())),
                0.0,
                color,
            );
        }
        let y = rect.bottom() - rect.height() * target / top;
        painter.hline(rect.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(60)));
        if n > 0 {
            let last = self.frame_ms[(self.frame_at + HISTORY - 1) % HISTORY];
            response.on_hover_text(format!("frame time; last {last:.2} ms, line at 60 fps"));
        }
    }

    fn resources(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let names: Vec<String> = ctx.world.reflected_resources().into_iter().map(str::to_owned).collect();
        for name in names {
            let Ok(mut value) = ctx.world.resource_json(&name) else {
                ui.label(RichText::new(format!("{name} (not inserted)")).weak());
                continue;
            };
            CollapsingHeader::new(&name)
                .id_salt(("resource", &name))
                .show(ui, |ui| {
                    if edit_value(ui, &mut value) {
                        let set = ctx.edit(|ed, ctx| ed.set_resource_merged(ctx, &name, value));
                        self.report(set);
                    }
                });
        }
    }

    fn entity_list(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let world = &ctx.world;
        ui.horizontal(|ui| {
            ui.label("With");
            let label = self
                .filter
                .map_or("anything".to_owned(), |id| component_name(world, id));
            egui::ComboBox::from_id_salt("inspector-filter")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, None, "anything");
                    for (i, info) in world.components().iter().enumerate() {
                        let Some(id) = world.component_at(i) else { continue };
                        ui.selectable_value(&mut self.filter, Some(id), short_name(info.name()));
                    }
                });
            let pick = ui
                .selectable_label(self.picking, "Pick")
                .on_hover_text("Click a unit in the world to select it");
            if pick.clicked() {
                self.picking = !self.picking;
            }
        });
        let total = list_rows(world, self.filter, &mut self.rows);
        ui.label(RichText::new(format!("{total} matching")).weak());
        let mut clicked = None;
        ScrollArea::vertical()
            .id_salt("inspector-entities")
            .max_height(220.0)
            .auto_shrink([false, true])
            .show_rows(ui, ROW_HEIGHT, total, |ui, range| {
                for row in range {
                    let Some(e) = entity_at(world, &self.rows, row) else {
                        continue;
                    };
                    let text = format!("{e:?}  {}", components_label(world, e));
                    let selected = self.selected == Some(e) || self.others.contains(&e);
                    if ui.selectable_label(selected, text).clicked() {
                        clicked = Some(e);
                    }
                }
            });
        if clicked.is_some() {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            self.select(clicked, add);
        }
    }

    fn entity(&mut self, ui: &mut Ui, ctx: &mut Ctx, e: Entity) {
        let position = ctx.world.get::<Transform2D>(e).map(|t| t.position);
        ui.horizontal(|ui| {
            ui.heading(format!("{e:?}"));
            if let Some(at) = position
                && ui.button("Look at").clicked()
            {
                ctx.camera.look_at(at);
            }
            if ui.button("Duplicate").on_hover_text("Ctrl+D").clicked() {
                let copy = ctx.edit(|ed, ctx| ed.duplicate(ctx, e, DUPLICATE_OFFSET));
                if let Some(copy) = self.report(copy) {
                    self.select(Some(copy), false);
                }
            }
            if ui.button("Despawn").on_hover_text("Del").clicked() {
                let deleted = ctx.edit(|ed, ctx| ed.delete(ctx, e));
                self.report(deleted);
                self.select(None, false);
            }
        });
        if self.selected.is_none() {
            return;
        }
        let ids: Vec<ComponentId> = ctx.world.components_of(e).map(<[_]>::to_vec).unwrap_or_default();
        let has: HashSet<String> = ids.iter().map(|&id| component_name(&ctx.world, id)).collect();
        for id in ids {
            let name = component_name(&ctx.world, id);
            if !self.reflected.contains(&name) {
                ui.label(RichText::new(format!("{name} (not reflected)")).weak())
                    .on_hover_text("world.reflect::<T>() makes it readable and editable here");
                continue;
            }
            let Ok(mut value) = ctx.world.get_json(e, &name) else {
                continue;
            };
            let mut remove = false;
            CollapsingHeader::new(&name)
                .id_salt(("component", &name))
                .default_open(true)
                .show(ui, |ui| {
                    if edit_value(ui, &mut value) {
                        let set = ctx.edit(|ed, ctx| ed.set_merged(ctx, e, &name, value));
                        self.report(set);
                    }
                    remove = ui.small_button("Remove").clicked();
                });
            if remove {
                let removed = ctx.edit(|ed, ctx| ed.remove(ctx, e, &name));
                self.report(removed);
            }
        }
        let mut addable: Vec<&String> = self.reflected.iter().filter(|n| !has.contains(*n)).collect();
        addable.sort();
        let mut add = None;
        egui::ComboBox::from_id_salt("inspector-add")
            .selected_text("Add component")
            .show_ui(ui, |ui| {
                for name in addable {
                    if ui.selectable_label(false, name).clicked() {
                        add = Some(name.clone());
                    }
                }
            });
        if let Some(name) = add {
            let added = ctx.edit(|ed, ctx| ed.add(ctx, e, &name));
            self.report(added);
        }
        crate::ai::debug_view::show(ui, &ctx.world, e);
    }

    /// A ring around each selected entity, in the world (the primary one
    /// stronger).
    fn highlight(&self, ui: &Ui, ctx: &Ctx) {
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::unique("inspector-highlight"),
            ))
            // Only over the world: never over the panels around it.
            .with_clip_rect(ui.available_rect_before_wrap());
        for (i, e) in self.selection().into_iter().enumerate() {
            let Some(t) = ctx.world.get::<Transform2D>(e) else {
                continue;
            };
            let at = ctx.camera.world_to_screen(t.position);
            let center = egui::pos2(at.x, at.y);
            let width = if i == 0 { 2.0 } else { 1.0 };
            painter.circle_stroke(center, 14.0, Stroke::new(width, Color32::from_rgb(255, 200, 40)));
            painter.circle_stroke(center, 16.0, Stroke::new(1.0, Color32::from_black_alpha(160)));
        }
        if self.editing {
            self.draw_gizmo2(&painter, ctx);
        }
    }
}

/// Short name of component `id`, as reflection and the control endpoint name it.
fn component_name(world: &World, id: ComponentId) -> String {
    short_name(world.component_info(id).name())
}

/// The components of `e`, comma separated.
fn components_label(world: &World, e: Entity) -> String {
    let Some(ids) = world.components_of(e) else {
        return String::new();
    };
    let mut out = String::new();
    for (i, &id) in ids.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&component_name(world, id));
    }
    out
}

/// Fills `rows` with each non-empty archetype that has `filter` and the row
/// its entities start at. Returns the number of rows.
fn list_rows(world: &World, filter: Option<ComponentId>, rows: &mut Vec<(usize, usize)>) -> usize {
    rows.clear();
    let mut total = 0;
    for (i, arch) in world.raw_ref().archetypes().iter().enumerate() {
        if arch.is_empty() || filter.is_some_and(|id| !arch.has(id)) {
            continue;
        }
        rows.push((i, total));
        total += arch.len();
    }
    total
}

/// The entity on `row` of a list made by [`list_rows`].
fn entity_at(world: &World, rows: &[(usize, usize)], row: usize) -> Option<Entity> {
    let k = rows.partition_point(|&(_, start)| start <= row).checked_sub(1)?;
    let (arch, start) = rows[k];
    world.raw_ref().archetypes()[arch].entities().get(row - start).copied()
}

/// The entity under window point `at`: the top sprite there (by `z`), else
/// the entity with a `Transform2D` closest to it, within [`PICK_RADIUS`]
/// pixels. Sprites are hit by their unrotated rectangle.
fn pick(ctx: &mut Ctx, at: Vec2) -> Option<Entity> {
    let camera = *crate::d2::View::camera_at(&ctx.camera, &ctx.views2d, at);
    let world_at = camera.screen_to_world(at);
    let mut top: Option<(f32, Entity)> = None;
    for (e, t, s) in ctx.world.query::<(Entity, &Transform2D, &Sprite)>().iter() {
        let size = s.region.map_or(s.texture.size(), |r| r.size()) * t.scale.abs();
        let min = t.position - size * s.pivot;
        let rect = Rect { min, max: min + size };
        if rect.contains(world_at) && top.is_none_or(|(z, _)| s.z >= z) {
            top = Some((s.z, e));
        }
    }
    if let Some((_, e)) = top {
        return Some(e);
    }
    let mut best: Option<(f32, Entity)> = None;
    for (e, t) in ctx.world.query::<(Entity, &Transform2D)>().iter() {
        let d = camera.world_to_screen(t.position).distance_squared(at);
        if d <= PICK_RADIUS * PICK_RADIUS && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, e));
        }
    }
    best.map(|(_, e)| e)
}

/// Edits a JSON value in place with the widget that fits it. Returns whether it changed.
fn edit_value(ui: &mut Ui, value: &mut Value) -> bool {
    match value {
        Value::Null => {
            ui.label(RichText::new("none").weak());
            false
        }
        Value::Bool(b) => ui.checkbox(b, "").changed(),
        Value::Number(n) => edit_number(ui, n),
        Value::String(s) => {
            // Applied when editing ends, not per key: a half-typed enum
            // variant ("Blu") would be refused and overwritten.
            let id = ui.next_auto_id();
            let mut draft = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| s.clone());
            let response = ui.text_edit_singleline(&mut draft);
            if response.has_focus() {
                ui.data_mut(|d| d.insert_temp(id, draft));
                false
            } else {
                ui.data_mut(|d| d.remove::<String>(id));
                let changed = response.lost_focus() && draft != *s;
                if changed {
                    *s = draft;
                }
                changed
            }
        }
        Value::Array(items) if items.len() <= 4 && items.iter().all(Value::is_number) => {
            // Vectors and colors: one row.
            let mut changed = false;
            ui.horizontal(|ui| {
                for item in items.iter_mut() {
                    if let Value::Number(n) = item {
                        changed |= edit_number(ui, n);
                    }
                }
            });
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            CollapsingHeader::new(format!("[{}]", items.len()))
                .id_salt(ui.next_auto_id())
                .show(ui, |ui| {
                    for (i, item) in items.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(i.to_string());
                                changed |= edit_value(ui, item);
                            });
                        });
                    }
                });
            changed
        }
        Value::Object(fields) => {
            let mut changed = false;
            egui::Grid::new(ui.next_auto_id())
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (key, field) in fields.iter_mut() {
                        ui.label(key);
                        ui.push_id(key, |ui| changed |= edit_value(ui, field));
                        ui.end_row();
                    }
                });
            changed
        }
    }
}

/// Keeps the number's kind: unsigned stays unsigned, integers stay integers.
fn edit_number(ui: &mut Ui, n: &mut serde_json::Number) -> bool {
    if let Some(mut v) = n.as_u64() {
        let changed = ui.add(DragValue::new(&mut v)).changed();
        *n = v.into();
        changed
    } else if let Some(mut v) = n.as_i64() {
        let changed = ui.add(DragValue::new(&mut v)).changed();
        *n = v.into();
        changed
    } else {
        let mut v = n.as_f64().unwrap_or(0.0);
        let speed = (v.abs() * 0.01).clamp(0.001, 10.0);
        let changed = ui.add(DragValue::new(&mut v).speed(speed)).changed();
        if let Some(f) = serde_json::Number::from_f64(v) {
            *n = f;
        }
        changed
    }
}

/// Each system's time this frame, slowest first.
fn systems(ui: &mut Ui, ctx: &Ctx) {
    let timings = ctx.perf.system_timings();
    if timings.is_empty() {
        ui.label(RichText::new("no systems").weak());
        return;
    }
    let top = timings.iter().map(|t| t.avg_ms).fold(0.001, f32::max);
    egui::Grid::new("inspector-systems").num_columns(3).show(ui, |ui| {
        for t in timings {
            let name = t.name.rsplit("::").next().unwrap_or(t.name);
            ui.label(if t.sim {
                format!("{name} (sim)")
            } else {
                name.to_owned()
            })
            .on_hover_text(t.name);
            ui.label(format!("{:.3} ms", t.avg_ms));
            let (rect, _) = ui.allocate_exact_size(vec2(60.0, 8.0), Sense::hover());
            let mut bar = rect;
            bar.set_width(rect.width() * t.avg_ms / top);
            ui.painter().rect_filled(bar, 1.0, Color32::from_rgb(242, 140, 40));
            ui.end_row();
        }
    });
}

/// One row of the GPU pass list: a pass's name and its time, or `None` when
/// it ran but couldn't be timed (shown as "not timed", never as 0 ms).
type GpuPassRow = (&'static str, Option<f32>);

/// The latest timed frame's GPU passes in the order they run, then the
/// passes that ran but couldn't be timed.
fn gpu_pass_rows(passes: &[crate::perf::GpuPass], not_timed: &[&'static str]) -> Vec<GpuPassRow> {
    let timed = passes.iter().map(|p| (p.name, Some(p.ms)));
    timed.chain(not_timed.iter().map(|&name| (name, None))).collect()
}

/// Each GPU pass's time; a pass that ran but couldn't be timed says
/// "not timed". Empty until a frame's times have come back (windowed games
/// time one frame in 16 unless profiling is on). When the latest frame has
/// no usable GPU time, it says why ([`Perf::gpu_untimed`](crate::perf::Perf::gpu_untimed)).
fn gpu_passes(ui: &mut Ui, ctx: &Ctx) {
    let rows = gpu_pass_rows(ctx.perf.gpu_passes(), ctx.perf.gpu_passes_not_timed());
    let untimed = ctx.perf.gpu_untimed();
    if rows.is_empty() {
        let text = match untimed {
            Some(why) => format!("no GPU pass times: {why}"),
            None => "no GPU pass times yet".to_owned(),
        };
        ui.label(RichText::new(text).weak());
        return;
    }
    if let Some(why) = untimed {
        ui.label(RichText::new(format!("from an earlier frame: {why}")).weak());
    }
    let top = rows.iter().filter_map(|r| r.1).fold(0.001, f32::max);
    egui::Grid::new("inspector-gpu-passes").num_columns(3).show(ui, |ui| {
        for (name, ms) in rows {
            ui.label(name);
            match ms {
                Some(ms) => {
                    ui.label(format!("{ms:.3} ms"));
                    let (rect, _) = ui.allocate_exact_size(vec2(60.0, 8.0), Sense::hover());
                    let mut bar = rect;
                    bar.set_width(rect.width() * ms / top);
                    ui.painter().rect_filled(bar, 1.0, Color32::from_rgb(90, 150, 230));
                }
                None => {
                    ui.label(RichText::new("not timed").weak()).on_hover_text(
                        "the pass ran, but the GPU wrote no usable timestamps for it (a pass with nothing to do, or a backend that can't time that kind of pass); the frame's GPU time still includes it",
                    );
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
}

/// The overlay as part of the windowed loop: egui's context and window
/// input, around [`Panels`].
pub(crate) struct Inspector {
    pub open: bool,
    egui: egui::Context,
    input: egui_winit::State,
    pub panels: Panels,
}

impl Inspector {
    pub fn new(window: &winit::window::Window, max_texture_side: usize) -> Self {
        let egui = egui::Context::default();
        let input = egui_winit::State::new(
            egui.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );
        Self {
            open: false,
            egui,
            input,
            panels: Panels::default(),
        }
    }

    /// Feeds a window event to egui. Returns `true` if the game shouldn't
    /// see it: egui uses it, or it's the click that picks an entity.
    pub fn on_event(&mut self, window: &winit::window::Window, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        if let WindowEvent::Focused(focused) = event {
            // Also while closed: the panels forget what was held when
            // they're opened again.
            self.panels.focus_changed(*focused);
        }
        if !self.open {
            return false;
        }
        if let WindowEvent::Focused(_) = event {
            self.forget_modifiers(window);
        }
        let consumed = self.input.on_window_event(window, event).consumed;
        if self.panels.picking
            && !self.egui.is_pointer_over_egui()
            && let WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } = event
        {
            let at = self.egui.input(|i| i.pointer.latest_pos());
            self.panels.pick_at = at.map(|p| Vec2::new(p.x, p.y));
            return true;
        }
        let input = matches!(
            event,
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
        );
        if self.panels.editing && input {
            // Paused: the editor has the mouse in the world, and its keys
            // (in edit mode every key: the game waits for Play).
            let mouse = !matches!(event, WindowEvent::KeyboardInput { .. });
            return consumed || mouse || self.panels.is_app() || self.is_editor_key(event);
        }
        consumed && input
    }

    /// Whether `event` is one of the editor's keys (see `Panels::shortcuts`).
    fn is_editor_key(&self, event: &winit::event::WindowEvent) -> bool {
        use winit::keyboard::{KeyCode, PhysicalKey};
        let winit::event::WindowEvent::KeyboardInput { event: key, .. } = event else {
            return false;
        };
        let PhysicalKey::Code(code) = key.physical_key else {
            return false;
        };
        let command = self.egui.input(|i| i.modifiers.command);
        match code {
            KeyCode::Delete | KeyCode::Backspace | KeyCode::Escape => true,
            KeyCode::KeyD | KeyCode::KeyZ => command,
            _ => self.panels.scene.claims_key(code, command),
        }
    }

    /// Runs the panels for this frame and hands their drawing to the renderer.
    pub fn frame(&mut self, window: &winit::window::Window, ctx: &mut Ctx) {
        ctx.ui.overlay_has_pointer = false;
        if !self.open {
            return;
        }
        let raw = self.input.take_egui_input(window);
        let panels = &mut self.panels;
        let output = panels.run(&self.egui, raw, ctx);
        ctx.ui.overlay_has_pointer = self.egui.is_pointer_over_egui();
        self.input.handle_platform_output(window, output.platform_output);
        let primitives = self.egui.tessellate(output.shapes, output.pixels_per_point);
        if let Some(renderer) = ctx.gfx.renderer.as_mut() {
            renderer.set_overlay(Overlay {
                primitives,
                textures: output.textures_delta,
                pixels_per_point: output.pixels_per_point,
            });
        }
        #[cfg(feature = "editor")]
        if panels.run_file_picker(ctx, |request, folder, name| {
            file_picker::system_picker(window, request, folder, name)
        }) {
            // The dialog had the keyboard (focus events may not say so);
            // `run_file_picker` told the panels to forget what was held.
            self.forget_modifiers(window);
        }
    }

    /// With [`Panels::input_lost`]: egui-winit stamps the modifiers it saw
    /// last on every key event, so they're released there too.
    fn forget_modifiers(&mut self, window: &winit::window::Window) {
        let released = winit::event::WindowEvent::ModifiersChanged(winit::event::Modifiers::default());
        let _ = self.input.on_window_event(window, &released);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key;
    use ponko_ecs::Component;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Serialize, Deserialize, Component)]
    struct Health {
        current: f32,
        max: u32,
    }
    #[derive(Component)]
    struct Secret;

    fn run(panels: &mut Panels, ctx: &mut Ctx) -> egui::FullOutput {
        let egui = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0))),
            ..Default::default()
        };
        let mut out = egui.run_ui(input, |ui| panels.show(ui, ctx));
        out.textures_delta.clear();
        out
    }

    fn game() -> (Ctx, Entity) {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(1280.0, 720.0));
        ctx.world.reflect::<Health>();
        ctx.world.reflect::<Transform2D>();
        let e = ctx.world.spawn((
            Transform2D::at(Vec2::new(10.0, 0.0)),
            Health { current: 3.0, max: 10 },
            Secret,
        ));
        for i in 0..100 {
            ctx.world.spawn((Transform2D::at(Vec2::new(i as f32 * 100.0, 500.0)),));
        }
        (ctx, e)
    }

    /// Runs one frame of `panels` on a lasting egui context (pointer state
    /// carries over between frames) with these input events.
    fn frame(egui: &egui::Context, panels: &mut Panels, ctx: &mut Ctx, mut events: Vec<egui::Event>) {
        // egui keeps held modifiers from ModifiersChanged, not from key events.
        let command = events
            .iter()
            .any(|e| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.command));
        let held = if command {
            egui::Modifiers::COMMAND
        } else {
            Default::default()
        };
        events.insert(0, egui::Event::ModifiersChanged(held));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0))),
            events,
            ..Default::default()
        };
        let mut out = egui.run_ui(input, |ui| panels.show(ui, ctx));
        out.textures_delta.clear();
    }
    fn button(x: f32, y: f32, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    fn key(key: Key, command: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if command {
                egui::Modifiers::COMMAND
            } else {
                Default::default()
            },
        }
    }
    fn edit_scene() -> (egui::Context, Panels, Ctx, Entity) {
        let (mut ctx, _) = game();
        ctx.time.pause();
        let e = ctx.world.spawn((
            Transform2D::at(Vec2::new(300.0, 300.0)),
            Health { current: 1.0, max: 1 },
        ));
        let egui = egui::Context::default();
        let mut panels = Panels::default();
        frame(&egui, &mut panels, &mut ctx, vec![]); // lay the panels out once
        (egui, panels, ctx, e)
    }

    #[test]
    fn paused_a_click_selects_a_drag_moves_and_undo_puts_it_back() {
        let (egui, mut panels, mut ctx, e) = edit_scene();
        let at = |ctx: &Ctx| ctx.world.get::<Transform2D>(e).unwrap().position;
        frame(
            &egui,
            &mut panels,
            &mut ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(305.0, 302.0)),
                button(305.0, 302.0, true),
            ],
        );
        assert_eq!(panels.selected, Some(e));
        frame(
            &egui,
            &mut panels,
            &mut ctx,
            vec![egui::Event::PointerMoved(egui::pos2(355.0, 322.0))],
        );
        assert_eq!(
            at(&ctx),
            Vec2::new(350.0, 320.0),
            "moved by the drag, not snapped to the cursor"
        );
        frame(&egui, &mut panels, &mut ctx, vec![button(355.0, 322.0, false)]);
        assert!(ctx.editor.can_undo());
        frame(&egui, &mut panels, &mut ctx, vec![key(Key::Z, true)]);
        assert_eq!(at(&ctx), Vec2::new(300.0, 300.0));
    }

    #[test]
    fn paused_keys_duplicate_and_delete_the_selection() {
        let (egui, mut panels, mut ctx, e) = edit_scene();
        panels.selected = Some(e);
        let before = ctx.world.len();
        frame(&egui, &mut panels, &mut ctx, vec![key(Key::D, true)]);
        let copy = panels.selected.unwrap();
        assert_ne!(copy, e);
        assert_eq!(
            ctx.world.get::<Transform2D>(copy).unwrap().position,
            Vec2::new(316.0, 316.0)
        );
        frame(&egui, &mut panels, &mut ctx, vec![key(Key::Delete, false)]);
        assert!(!ctx.world.is_alive(copy) && panels.selected.is_none());
        assert_eq!(ctx.world.len(), before);
    }

    #[test]
    fn running_the_world_clicks_go_to_the_game() {
        let (egui, mut panels, mut ctx, e) = edit_scene();
        ctx.time.resume();
        frame(
            &egui,
            &mut panels,
            &mut ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(300.0, 300.0)),
                button(300.0, 300.0, true),
            ],
        );
        assert_eq!(panels.selected, None);
        assert!(!panels.editing);
        let _ = e;
    }

    #[test]
    fn a_scene_load_clears_the_selection() {
        let (egui, mut panels, mut ctx, e) = edit_scene();
        panels.selected = Some(e);
        frame(&egui, &mut panels, &mut ctx, vec![]);
        assert_eq!(panels.selected, Some(e));
        // The same ids again, from a load: `e` may be another entity now.
        let save = WorldReflect::save(&ctx.world);
        ctx.replace_world(&save).unwrap();
        assert!(ctx.world.is_alive(e));
        frame(&egui, &mut panels, &mut ctx, vec![]);
        assert_eq!(panels.selected, None, "the selection named the old world's entity");
        panels.selected = Some(e);
        frame(&egui, &mut panels, &mut ctx, vec![]);
        assert_eq!(panels.selected, Some(e), "a new selection stays");
    }

    #[test]
    fn play_then_stop_from_the_panels_restores_the_world() {
        let (egui, mut panels, mut ctx, e) = edit_scene();
        ctx.edit(|ed, ctx| ed.play(ctx));
        ctx.world.despawn(e);
        frame(&egui, &mut panels, &mut ctx, vec![]);
        ctx.edit(|ed, ctx| ed.stop(ctx)).unwrap();
        assert!(
            ctx.world.get::<Health>(e).is_some(),
            "back, with the same id (the world was reloaded)"
        );
    }

    #[test]
    fn panels_draw_a_selected_entity_with_reflected_and_hidden_components() {
        let (mut ctx, e) = game();
        let mut panels = Panels {
            selected: Some(e),
            ..Default::default()
        };
        let out = run(&mut panels, &mut ctx);
        assert!(!out.shapes.is_empty());
        assert_eq!(panels.selected, Some(e));
        assert!(panels.error.is_none());
    }

    #[test]
    fn a_despawned_selection_is_dropped() {
        let (mut ctx, e) = game();
        let mut panels = Panels {
            selected: Some(e),
            ..Default::default()
        };
        ctx.world.despawn(e);
        let _ = run(&mut panels, &mut ctx);
        assert_eq!(panels.selected, None);
    }

    #[test]
    fn rows_cover_every_matching_entity_once() {
        let (ctx, e) = game();
        let mut rows = Vec::new();
        let total = list_rows(&ctx.world, None, &mut rows);
        assert_eq!(total, 101);
        let listed: HashSet<Entity> = (0..total).filter_map(|r| entity_at(&ctx.world, &rows, r)).collect();
        assert_eq!(listed.len(), 101);
        assert!(listed.contains(&e));
        assert_eq!(entity_at(&ctx.world, &rows, total), None);

        let health = ctx
            .world
            .raw_ref()
            .component_by_name(std::any::type_name::<Health>())
            .unwrap();
        assert_eq!(list_rows(&ctx.world, Some(health), &mut rows), 1);
        assert_eq!(entity_at(&ctx.world, &rows, 0), Some(e));
    }

    #[test]
    fn a_pick_selects_the_nearest_entity_under_the_click() {
        let (mut ctx, e) = game();
        let at = ctx.camera.world_to_screen(Vec2::new(12.0, 3.0));
        let mut panels = Panels {
            picking: true,
            pick_at: Some(at),
            ..Default::default()
        };
        let _ = run(&mut panels, &mut ctx);
        assert_eq!(panels.selected, Some(e));
        assert!(!panels.picking, "picking ends after one click");

        let far = ctx.camera.world_to_screen(Vec2::new(55.0, -300.0));
        assert_eq!(pick(&mut ctx, far), None, "nothing within reach");
    }

    #[test]
    fn number_edits_keep_their_kind() {
        let egui = egui::Context::default();
        let mut value = json!({ "current": 2.5, "max": 7, "offset": -3, "name": "crab", "on": true });
        egui.run_ui(egui::RawInput::default(), |ui| {
            assert!(!edit_value(ui, &mut value), "nothing changes without input");
        })
        .drop_without_applying_deltas();
        assert_eq!(
            value,
            json!({ "current": 2.5, "max": 7, "offset": -3, "name": "crab", "on": true })
        );
        assert!(value["max"].is_u64() && value["offset"].is_i64() && value["current"].is_f64());
    }

    /// Every text drawn on the screen of one headless egui frame.
    fn drawn_texts(ctx: &Ctx) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let egui = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut out = egui.run_ui(input, |ui| gpu_passes(ui, ctx));
        out.textures_delta.clear();
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
        texts
    }

    #[test]
    fn gpu_passes_that_ran_but_could_not_be_timed_show_as_not_timed_not_as_zero() {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(400.0, 300.0));
        // As in a windowed game whose latest frame was timed.
        ctx.perf.gpu_untimed = None;
        ctx.perf.gpu_passes = vec![
            crate::perf::GpuPass {
                name: "opaque",
                ms: 1.5,
            },
            crate::perf::GpuPass { name: "post", ms: 0.25 },
        ];
        ctx.perf.gpu_passes_not_timed = vec!["crowd cull"];
        let texts = drawn_texts(&ctx);
        assert!(texts.contains(&"1.500 ms".to_owned()), "{texts:?}");
        assert!(texts.contains(&"0.250 ms".to_owned()), "{texts:?}");
        let at = |name: &str| {
            texts
                .iter()
                .position(|t| t == name)
                .unwrap_or_else(|| panic!("{name} missing: {texts:?}"))
        };
        assert_eq!(texts[at("crowd cull") + 1], "not timed", "{texts:?}");
        assert!(at("crowd cull") > at("post"), "timed passes come first: {texts:?}");
        assert_eq!(
            texts.iter().filter(|t| t.as_str() == "not timed").count(),
            1,
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.starts_with("0.000")), "no 0 ms row: {texts:?}");

        // Nothing came back yet: a plain note, no rows.
        ctx.perf.gpu_passes.clear();
        ctx.perf.gpu_passes_not_timed.clear();
        assert_eq!(drawn_texts(&ctx), ["no GPU pass times yet"]);
    }

    /// No GPU time: the panel says why, with rows or without.
    #[test]
    fn the_gpu_panel_says_why_a_frame_has_no_gpu_time() {
        use crate::perf::GpuUntimed;
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(400.0, 300.0));
        ctx.perf.gpu_untimed = Some(GpuUntimed::QuerySetBudget);
        assert_eq!(
            drawn_texts(&ctx),
            [format!("no GPU pass times: {}", GpuUntimed::QuerySetBudget.message())]
        );
        ctx.perf.gpu_passes = vec![crate::perf::GpuPass {
            name: "opaque",
            ms: 1.5,
        }];
        ctx.perf.gpu_untimed = Some(GpuUntimed::DeviceLost);
        let texts = drawn_texts(&ctx);
        assert_eq!(
            texts[0],
            format!("from an earlier frame: {}", GpuUntimed::DeviceLost.message())
        );
        assert!(texts.contains(&"1.500 ms".to_owned()), "{texts:?}");
    }

    #[test]
    fn frame_history_wraps_around() {
        let mut panels = Panels::default();
        for i in 0..HISTORY + 10 {
            panels.record_frame(i as f32);
        }
        assert_eq!(panels.frame_ms.len(), HISTORY);
        assert_eq!(panels.frame_at, 10);
        assert_eq!(panels.frame_ms[9], (HISTORY + 9) as f32);
    }
}
