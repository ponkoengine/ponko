//! The editor's authoring windows, for data tuned by eye that lives in its
//! own files: Tiled maps, animation graphs, materials and shaders, terrain.
//! The **Author** menu opens a file in one of them; each window edits its
//! file as a document of the editor's history (see `editor_author.rs`), so
//! Ctrl+Z and Ctrl+S cover it, and saves it in the file's own format.
//! Particle emitters are components: their section is in the inspector
//! (`inspector_particles.rs`).
//!
//! This file holds what the windows share: their state, the menu, the
//! dialog that picks a file, and the hooks the editor's frame calls.

use egui::{RichText, Ui};
use glam::Vec2;

use super::Panels;
use crate::ctx::Ctx;

/// The kinds of file the authoring windows edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Tilemap,
    Graph,
    #[cfg(feature = "3d")]
    Materials,
    #[cfg(feature = "3d")]
    Shader,
    #[cfg(feature = "3d")]
    Terrain,
}

impl Tool {
    /// Every tool, in the menu's order.
    pub(crate) fn all() -> &'static [Tool] {
        &[
            Tool::Tilemap,
            Tool::Graph,
            #[cfg(feature = "3d")]
            Tool::Materials,
            #[cfg(feature = "3d")]
            Tool::Shader,
            #[cfg(feature = "3d")]
            Tool::Terrain,
        ]
    }

    /// The menu entry.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Tilemap => "Tilemap…",
            Tool::Graph => "Animation graph…",
            #[cfg(feature = "3d")]
            Tool::Materials => "Materials…",
            #[cfg(feature = "3d")]
            Tool::Shader => "Shader…",
            #[cfg(feature = "3d")]
            Tool::Terrain => "Terrain…",
        }
    }

    /// What it edits, for the menu's tooltip and the picker.
    fn what(self) -> &'static str {
        match self {
            Tool::Tilemap => "a Tiled map (.tmj): paint its tile layers, with autotiling",
            Tool::Graph => "a 2D skeleton file's animation graph (.json)",
            #[cfg(feature = "3d")]
            Tool::Materials => "a glTF file's materials (.gltf)",
            #[cfg(feature = "3d")]
            Tool::Shader => "a custom WGSL shader (.wgsl), with its errors in place",
            #[cfg(feature = "3d")]
            Tool::Terrain => "a heightmap (16-bit greyscale .png) and its weight map",
        }
    }

    /// The files in `assets/` it can open.
    fn candidates(self) -> Vec<String> {
        let ext = match self {
            Tool::Tilemap => "tmj",
            Tool::Graph => "json",
            #[cfg(feature = "3d")]
            Tool::Materials => "gltf",
            #[cfg(feature = "3d")]
            Tool::Shader => "wgsl",
            #[cfg(feature = "3d")]
            Tool::Terrain => "png",
        };
        let mut found = crate::assets::list("", ext);
        found.retain(|p| match self {
            Tool::Graph => super::graph_panel::is_skeleton_file(p),
            #[cfg(feature = "3d")]
            Tool::Terrain => super::terrain_panel::is_heightmap_file(p),
            _ => true,
        });
        found.sort();
        found
    }
}

/// The authoring windows' state, kept by the editor app.
#[derive(Default)]
pub(crate) struct Authoring {
    /// The file picker: which tool, and the path typed so far.
    pub picking: Option<(Tool, String)>,
    /// What went wrong opening a file.
    pub error: Option<String>,
    pub tilemap: super::tilemap_panel::TilemapState,
    pub graph: super::graph_panel::GraphState,
    #[cfg(feature = "3d")]
    pub materials: super::material_panel::MaterialState,
    #[cfg(feature = "3d")]
    pub terrain: super::terrain_panel::TerrainState,
}

impl std::fmt::Debug for Authoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authoring")
            .field("picking", &self.picking)
            .finish_non_exhaustive()
    }
}

impl Panels {
    fn authoring(&mut self) -> Option<&mut Authoring> {
        self.app.as_mut().map(|a| &mut a.author)
    }

    /// The **Author** menu.
    pub(super) fn author_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Author", |ui| {
            for &tool in Tool::all() {
                if ui.button(tool.label()).on_hover_text(tool.what()).clicked() {
                    if let Some(a) = self.authoring() {
                        a.picking = Some((tool, String::new()));
                        a.error = None;
                    }
                    ui.close();
                }
            }
        });
    }

    /// Opens `path` in `tool`'s window.
    pub(crate) fn author_open(&mut self, ctx: &mut Ctx, tool: Tool, path: &str) -> Result<(), String> {
        let path = path.trim().trim_start_matches("assets/").to_string();
        match tool {
            Tool::Tilemap => self.open_tilemap(ctx, &path),
            Tool::Graph => self.open_graph(ctx, &path),
            #[cfg(feature = "3d")]
            Tool::Materials => self.open_materials(ctx, &path),
            #[cfg(feature = "3d")]
            Tool::Shader => self.open_shader(ctx, &path),
            #[cfg(feature = "3d")]
            Tool::Terrain => self.open_terrain(ctx, &path),
        }
    }

    /// The picker and every open authoring window, one frame.
    pub(super) fn author_windows(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        if self.app.is_none() {
            return;
        }
        self.author_picker(ui, ctx);
        self.tilemap_window(ui, ctx);
        self.graph_window(ui, ctx);
        #[cfg(feature = "3d")]
        {
            self.material_window(ui, ctx);
            self.shader_window(ui, ctx);
            self.terrain_window(ui, ctx);
        }
    }

    /// The pointer in the view, for a brush that paints there (a tile, the
    /// terrain). `true` when a brush took the press, so the view's own
    /// selection and gizmo leave it alone.
    pub(super) fn author_viewport(&mut self, ui: &Ui, ctx: &mut Ctx, pos: Vec2) -> bool {
        if self.app.is_none() || ui.ctx().is_pointer_over_egui() {
            return false;
        }
        if self.tilemap_brush(ui, ctx, pos) {
            return true;
        }
        #[cfg(feature = "3d")]
        if self.terrain_brush(ui, ctx, pos) {
            return true;
        }
        false
    }

    /// The dialog that picks a file for a tool: a path field over the
    /// files it can open.
    fn author_picker(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some((tool, mut path)) = self.authoring().and_then(|a| a.picking.clone()) else {
            return;
        };
        let mut open = None;
        let mut cancel = false;
        egui::Window::new(format!("Open {}", tool.label().trim_end_matches('…').to_lowercase()))
            .id(egui::Id::unique("ponko-author-picker"))
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.label(RichText::new(tool.what()).weak());
                let field = ui.add(egui::TextEdit::singleline(&mut path).hint_text("path in assets/"));
                if !field.has_focus() && path.is_empty() {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    open = Some(path.clone());
                }
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for file in tool.candidates() {
                        if ui.selectable_label(false, &file).clicked() {
                            open = Some(file);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        open = Some(path.clone());
                    }
                    cancel = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
                if let Some(error) = self.app.as_ref().and_then(|a| a.author.error.clone()) {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            });
        if cancel {
            if let Some(a) = self.authoring() {
                a.picking = None;
            }
            return;
        }
        if let Some(file) = open {
            let opened = self.author_open(ctx, tool, &file);
            if let Some(a) = self.authoring() {
                match opened {
                    Ok(()) => {
                        a.picking = None;
                        a.error = None;
                    }
                    Err(e) => a.error = Some(e),
                }
            }
            return;
        }
        if let Some(a) = self.authoring() {
            a.picking = Some((tool, path));
        }
    }
}

/// Reads asset `path` as text.
pub(crate) fn read_text(ctx: &Ctx, path: &str) -> Result<String, String> {
    ctx.assets.read_text(path).map(|(_, t)| t).map_err(|e| e.to_string())
}

/// Reads asset `path`.
pub(crate) fn read_bytes(ctx: &Ctx, path: &str) -> Result<Vec<u8>, String> {
    ctx.assets
        .asset_sources
        .find(path)
        .and_then(|f| f.into_bytes())
        .map_err(|e| e.to_string())
}

/// Writes a JSON document as the file's text, laid out as Tiled and the
/// skeleton format write theirs (two-space indents).
pub(crate) fn write_json(path: &str, value: &serde_json::Value) -> Result<(), crate::error::Error> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|e| crate::error::Error::new(crate::error::ErrorKind::Invalid, e.to_string()))?;
    text.push('\n');
    crate::assets::write_asset(path, text.as_bytes())
}

/// `name` relative to the folder of the file at `path` (an asset name).
pub(crate) fn beside(path: &str, name: &str) -> String {
    crate::tilemap::resolve_relative_asset(path, name)
}
