//! The tilemap window: paints the tile layers of a Tiled map (`.tmj`) in
//! the 2D view. Pick a layer and a tile from the tileset's palette, then
//! paint or erase with the left mouse button over the level in the view;
//! the **Autotile** brush fills cells and gives each the frame its
//! neighbours call for, by the layer's rule (four neighbours, 16 tiles; or
//! the 47-tile blob), from the block of the tileset the rule starts at.
//!
//! The map is a document of the editor's history: a stroke is one undo
//! step, and Save writes the `.tmj` back with only what changed: the layers'
//! `data` (written as a plain list of tile numbers, Tiled's CSV layer
//! format) and the `autotile` property of a layer given a rule. The level
//! spawned from the file follows each change at once.

use egui::{Color32, Rect, RichText, Sense, Stroke, Ui};
use glam::Vec2;
use serde_json::{Value, json};

use super::Panels;
use super::author::{beside, read_bytes, read_text, write_json};
use crate::ctx::Ctx;
use crate::tilemap::{AutoTile, LevelRoot};

/// Tiled's flip flags, in a tile number's top bits.
const FLIPS: u32 = 0xE000_0000;

/// What the left mouse button does over the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Brush {
    #[default]
    Paint,
    Erase,
    Autotile,
}

/// The tilemap window's state.
#[derive(Default)]
pub(crate) struct TilemapState {
    /// The map being edited.
    pub path: Option<String>,
    /// The tile layer painted: its place among the map's tile layers.
    pub layer: usize,
    /// The tileset picked from (its place in the map's list).
    pub tileset: usize,
    /// The tile picked, within the tileset.
    pub tile: u32,
    pub brush: Brush,
    /// The palette's image, for the tileset it was made from.
    palette: Option<(String, egui::TextureHandle)>,
    /// The documents' version last applied to the level in the world.
    applied: u64,
    pub error: Option<String>,
}

/// A tileset as the palette and the brushes need it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sheet {
    pub firstgid: u32,
    pub image: String,
    pub columns: u32,
    pub count: u32,
    pub tile: (u32, u32),
    pub margin: u32,
    pub spacing: u32,
    pub size: (u32, u32),
}

/// Where the palette was drawn last (egui memory, for tests).
pub(crate) fn palette_id() -> egui::Id {
    egui::Id::unique("ponko-tile-palette")
}

/// Every tile layer of `map`, as the path of indices to it through groups.
pub(crate) fn tile_layers(map: &Value) -> Vec<Vec<usize>> {
    fn walk(list: &Value, prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, layer) in list.as_array().into_iter().flatten().enumerate() {
            prefix.push(i);
            match layer["type"].as_str() {
                Some("tilelayer") => out.push(prefix.clone()),
                Some("group") => walk(&layer["layers"], prefix, out),
                _ => {}
            }
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    walk(&map["layers"], &mut Vec::new(), &mut out);
    out
}

fn layer_at<'a>(map: &'a Value, path: &[usize]) -> &'a Value {
    let mut v = &map["layers"][path[0]];
    for &i in &path[1..] {
        v = &v["layers"][i];
    }
    v
}

fn layer_at_mut<'a>(map: &'a mut Value, path: &[usize]) -> &'a mut Value {
    let mut v = &mut map["layers"][path[0]];
    for &i in &path[1..] {
        v = &mut v["layers"][i];
    }
    v
}

/// The map as the window edits it: every tile layer's data a plain list of
/// tile numbers (base64 and compressed layers are decoded).
fn editable(mut map: Value, path: &str) -> Result<Value, String> {
    if map["infinite"].as_bool() == Some(true) {
        return Err(format!(
            "{path} is an infinite map: its layers are chunks; make it a fixed-size map in Tiled to paint it here"
        ));
    }
    if map["layers"].as_array().is_none() {
        return Err(format!("{path} isn't a Tiled JSON map (no \"layers\")"));
    }
    for at in tile_layers(&map) {
        let layer = layer_at_mut(&mut map, &at);
        let gids = crate::tilemap::tile_layer_gids(layer, &layer["data"], path).map_err(|e| e.to_string())?;
        let obj = layer.as_object_mut().ok_or("a layer isn't an object")?;
        obj.remove("encoding");
        obj.remove("compression");
        obj.insert("data".into(), json!(gids));
    }
    Ok(map)
}

/// The map's tilesets (external ones read from their files).
fn sheets(ctx: &Ctx, path: &str, map: &Value) -> Result<Vec<Sheet>, String> {
    let mut out = Vec::new();
    for t in map["tilesets"].as_array().into_iter().flatten() {
        let firstgid = t["firstgid"].as_u64().unwrap_or(1) as u32;
        let (set, base) = match t["source"].as_str() {
            Some(source) => {
                let file = beside(path, source);
                let text = read_text(ctx, &file)?;
                let set: Value = serde_json::from_str(&text)
                    .map_err(|e| format!("{file}: a tileset the tilemap window reads is JSON (.tsj): {e}"))?;
                (set, file)
            }
            None => (t.clone(), path.to_string()),
        };
        let Some(image) = set["image"].as_str() else {
            continue; // a collection of images: no grid to paint from
        };
        let n = |k: &str| set[k].as_u64().unwrap_or(0) as u32;
        out.push(Sheet {
            firstgid,
            image: beside(&base, image),
            columns: n("columns").max(1),
            count: n("tilecount"),
            tile: (n("tilewidth").max(1), n("tileheight").max(1)),
            margin: n("margin"),
            spacing: n("spacing"),
            size: (n("imagewidth"), n("imageheight")),
        });
    }
    Ok(out)
}

/// The rule in a layer's `autotile` property: `"four 16"` (16 tiles from
/// tile 16 of the tileset, by mask) or `"blob 0"` (47 tiles from tile 0, in
/// `AutoTile::BLOB_MASKS` order).
pub(crate) fn rule_of(layer: &Value) -> Option<(String, u32)> {
    let spec = layer["properties"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == "autotile")?["value"]
        .as_str()?
        .to_string();
    let mut words = spec.split_whitespace();
    let kind = words.next()?.to_string();
    let first = words.next()?.parse().ok()?;
    matches!(kind.as_str(), "four" | "blob").then_some((kind, first))
}

fn rule(kind: &str, first: u32) -> AutoTile {
    let first = first as u16;
    if kind == "blob" {
        AutoTile::blob(std::array::from_fn(|i| first + i as u16))
    } else {
        AutoTile::four(std::array::from_fn(|m| first + m as u16))
    }
}

fn set_rule(layer: &mut Value, spec: Option<(String, u32)>) {
    let mut props: Vec<Value> = layer["properties"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p["name"] != "autotile")
        .collect();
    if let Some((kind, first)) = spec {
        props.push(json!({"name": "autotile", "type": "string", "value": format!("{kind} {first}")}));
    }
    if let Some(obj) = layer.as_object_mut() {
        if props.is_empty() {
            obj.remove("properties");
        } else {
            obj.insert("properties".into(), Value::Array(props));
        }
    }
}

/// Paints cell `(x, y)` of `layer` (a tile layer of a `width` × `height`
/// map) with `brush`: `gid` for Paint, empty for Erase, filled by the
/// layer's rule for Autotile. Autotiled neighbours (and an autotiled cell
/// erased) get the frames their neighbours call for.
pub(crate) fn paint(layer: &mut Value, sheet: &Sheet, brush: Brush, gid: u32, x: u32, y: u32) {
    let (w, h) = (
        layer["width"].as_u64().unwrap_or(0) as u32,
        layer["height"].as_u64().unwrap_or(0) as u32,
    );
    if x >= w || y >= h {
        return;
    }
    let spec = rule_of(layer);
    let Some(data) = layer["data"].as_array_mut() else {
        return;
    };
    let mut cells: Vec<u32> = data.iter().map(|v| v.as_u64().unwrap_or(0) as u32).collect();
    if cells.len() != (w * h) as usize {
        return;
    }
    let at = (y * w + x) as usize;
    let auto = spec.map(|(k, f)| rule(&k, f));
    let local = |gid: u32| {
        let g = gid & !FLIPS;
        (g >= sheet.firstgid && g < sheet.firstgid + sheet.count.max(1)).then(|| (g - sheet.firstgid) as u16)
    };
    match brush {
        Brush::Paint => cells[at] = gid,
        Brush::Erase => cells[at] = 0,
        Brush::Autotile => {
            let Some(auto) = &auto else { return };
            // Any tile the rule owns marks the cell filled; the frame is set below.
            cells[at] = sheet.firstgid + u32::from(auto.frame(0));
        }
    }
    if let Some(auto) = &auto {
        let filled =
            |cells: &[u32], cx: u32, cy: u32| local(cells[(cy * w + cx) as usize]).is_some_and(|f| auto.owns(f));
        let (x0, y0) = (x.saturating_sub(1), y.saturating_sub(1));
        // Which cells are filled doesn't change below, only their frames.
        let snapshot = cells.clone();
        for cy in y0..=(y + 1).min(h - 1) {
            for cx in x0..=(x + 1).min(w - 1) {
                if filled(&snapshot, cx, cy) {
                    let mask = auto.mask(cx, cy, w, h, |a, b| filled(&snapshot, a, b));
                    cells[(cy * w + cx) as usize] = sheet.firstgid + u32::from(auto.frame(mask));
                }
            }
        }
    }
    *data = cells.into_iter().map(Value::from).collect();
}

impl Panels {
    fn tilemap(&mut self) -> Option<&mut TilemapState> {
        self.app.as_mut().map(|a| &mut a.author.tilemap)
    }

    /// Opens the Tiled map at `path` in the tilemap window.
    pub(super) fn open_tilemap(&mut self, ctx: &mut Ctx, path: &str) -> Result<(), String> {
        let text = read_text(ctx, path)?;
        let map: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: not JSON: {e}"))?;
        let map = editable(map, path)?;
        if tile_layers(&map).is_empty() {
            return Err(format!("{path} has no tile layer to paint"));
        }
        if sheets(ctx, path, &map)?.is_empty() {
            return Err(format!("{path} has no tileset made of one image to paint from"));
        }
        ctx.editor.docs.open(path, map, write_json);
        let state = self.tilemap().ok_or("not the editor")?;
        *state = TilemapState {
            path: Some(path.to_string()),
            ..TilemapState::default()
        };
        Ok(())
    }

    /// The level spawned from the open map, and its top-left corner.
    fn tile_level(ctx: &mut Ctx, path: &str) -> Option<Vec2> {
        ctx.world
            .query::<(&crate::d2::Transform, &LevelRoot)>()
            .iter()
            .find(|(_, r)| r.path == path)
            .map(|(t, _)| t.position)
    }

    /// The tilemap window, and the level in the world following the map.
    pub(super) fn tilemap_window(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(state) = self.tilemap() else { return };
        let Some(path) = state.path.clone() else { return };
        let Some(map) = ctx.editor.docs.get(&path).cloned() else {
            state.path = None;
            return;
        };
        // The level follows the map (an edit, an undo, a redo).
        let version = ctx.editor.docs.version();
        if state.applied != version {
            state.applied = version;
            let text = serde_json::to_string(&map).unwrap_or_default();
            if Self::tile_level(ctx, &path).is_some()
                && let Err(e) = ctx.reload_level_from_text(&path, &text)
            {
                state.error = Some(e);
            }
        }
        let sheets = sheets(ctx, &path, &map).unwrap_or_default();
        let layers = tile_layers(&map);
        let unsaved = ctx.editor.docs.unsaved(&path);
        let mut open = true;
        let mut new_map = None;
        let mut save = false;
        let mut show = false;
        egui::Window::new(format!("Tilemap — {path}{}", if unsaved { " *" } else { "" }))
            .id(egui::Id::unique("ponko-tilemap-window"))
            .open(&mut open)
            .default_pos(egui::pos2(320.0, 80.0))
            .show(ui.ctx(), |ui| {
                let Some(state) = self.app.as_mut().map(|a| &mut a.author.tilemap) else {
                    return;
                };
                ui.horizontal(|ui| {
                    save = ui.add_enabled(unsaved, egui::Button::new("Save")).clicked();
                    if Self::tile_level(ctx, &path).is_none() {
                        show = ui
                            .button("Show in the view")
                            .on_hover_text(
                                "Spawn the level in the middle of the view to paint it (not saved with the scene)",
                            )
                            .clicked();
                    }
                });
                state.layer = state.layer.min(layers.len().saturating_sub(1));
                let name = |at: &Vec<usize>| layer_at(&map, at)["name"].as_str().unwrap_or("?").to_string();
                egui::ComboBox::from_label("Layer")
                    .selected_text(layers.get(state.layer).map(name).unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, at) in layers.iter().enumerate() {
                            ui.selectable_value(&mut state.layer, i, name(at));
                        }
                    });
                if sheets.len() > 1 {
                    egui::ComboBox::from_label("Tileset")
                        .selected_text(sheets[state.tileset.min(sheets.len() - 1)].image.clone())
                        .show_ui(ui, |ui| {
                            for (i, s) in sheets.iter().enumerate() {
                                ui.selectable_value(&mut state.tileset, i, &s.image);
                            }
                        });
                }
                state.tileset = state.tileset.min(sheets.len().saturating_sub(1));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.brush, Brush::Paint, "Paint");
                    ui.selectable_value(&mut state.brush, Brush::Erase, "Erase");
                    ui.selectable_value(&mut state.brush, Brush::Autotile, "Autotile");
                });
                let Some(at) = layers.get(state.layer) else { return };
                let layer = layer_at(&map, at);
                let spec = rule_of(layer);
                let mut kind = spec.as_ref().map_or("none".to_string(), |s| s.0.clone());
                let mut first = spec.as_ref().map_or(0, |s| s.1);
                let before = (kind.clone(), first);
                ui.horizontal(|ui| {
                    ui.label("Autotile rule");
                    egui::ComboBox::from_id_salt("ponko-autotile-rule")
                        .selected_text(&kind)
                        .show_ui(ui, |ui| {
                            for k in ["none", "four", "blob"] {
                                ui.selectable_value(&mut kind, k.to_string(), k);
                            }
                        });
                    if kind != "none" {
                        ui.label("from tile");
                        ui.add(egui::DragValue::new(&mut first).range(0..=u16::MAX as u32));
                    }
                })
                .response
                .on_hover_text(
                    "four: 16 tiles in mask order (N=1, E=2, S=4, W=8); blob: 47 tiles in AutoTile::BLOB_MASKS order",
                );
                if (kind.clone(), first) != before {
                    let mut m = map.clone();
                    set_rule(
                        layer_at_mut(&mut m, at),
                        (kind != "none").then_some((kind.clone(), first)),
                    );
                    new_map = Some(m);
                }
                if state.brush == Brush::Autotile && kind == "none" {
                    ui.label(RichText::new("Give the layer a rule to autotile").weak());
                }
                if let Some(sheet) = sheets.get(state.tileset) {
                    palette(ui, ctx, state, sheet);
                }
                ui.label(RichText::new("Left mouse over the level paints; right or middle drag pans the view").weak());
                if let Some(e) = &state.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            });
        if let Some(m) = new_map {
            ctx.editor.set_doc(&path, m);
        }
        if show {
            let size = Vec2::new(
                map["width"].as_f64().unwrap_or(0.0) as f32 * map["tilewidth"].as_f64().unwrap_or(0.0) as f32,
                map["height"].as_f64().unwrap_or(0.0) as f32 * map["tileheight"].as_f64().unwrap_or(0.0) as f32,
            );
            let at = ctx.camera.center() - size / 2.0;
            let spawned = ctx.spawn_level(&path, at).map(drop);
            self.report(spawned.map_err(|e| e.to_string()));
            if let Some(s) = self.tilemap() {
                s.applied = 0;
            }
        }
        if save {
            let saved = ctx.editor.docs.save(&path).map_err(|e| e.to_string());
            if self.report(saved).is_some() {
                self.status = Some(format!("saved {path}"));
            }
        }
        if !open {
            ctx.editor.docs.close(&path);
            if let Some(s) = self.tilemap() {
                *s = TilemapState::default();
            }
        }
    }

    /// The tile brush over the view: `true` while it has the mouse.
    pub(super) fn tilemap_brush(&mut self, ui: &Ui, ctx: &mut Ctx, pos: Vec2) -> bool {
        let Some(state) = self.app.as_ref().map(|a| &a.author.tilemap) else {
            return false;
        };
        let Some(path) = state.path.clone() else { return false };
        let (layer_i, tileset, tile, brush) = (state.layer, state.tileset, state.tile, state.brush);
        let Some(origin) = Self::tile_level(ctx, &path) else {
            return false;
        };
        let Some(mut map) = ctx.editor.docs.get(&path).cloned() else {
            return false;
        };
        let tw = map["tilewidth"].as_f64().unwrap_or(1.0) as f32;
        let th = map["tileheight"].as_f64().unwrap_or(1.0) as f32;
        let world = ctx.camera.screen_to_world(pos);
        let cell = ((world - origin) / Vec2::new(tw, th)).floor();
        let (w, h) = (
            map["width"].as_f64().unwrap_or(0.0) as f32,
            map["height"].as_f64().unwrap_or(0.0) as f32,
        );
        if cell.x < 0.0 || cell.y < 0.0 || cell.x >= w || cell.y >= h {
            return false;
        }
        let (pressed, down) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
        if !(pressed || down) {
            return false;
        }
        let layers = tile_layers(&map);
        let Some(at) = layers.get(layer_i) else { return false };
        let Ok(sheets) = sheets(ctx, &path, &map) else {
            return false;
        };
        let Some(sheet) = sheets.get(tileset) else { return false };
        paint(
            layer_at_mut(&mut map, at),
            sheet,
            brush,
            sheet.firstgid + tile,
            cell.x as u32,
            cell.y as u32,
        );
        ctx.editor.set_doc_merged(&path, map);
        true
    }
}

/// The tileset's palette: click a tile to pick it.
fn palette(ui: &mut Ui, ctx: &Ctx, state: &mut TilemapState, sheet: &Sheet) {
    if state.palette.as_ref().is_none_or(|(image, _)| *image != sheet.image) {
        let texture = read_bytes(ctx, &sheet.image)
            .and_then(|bytes| {
                crate::assets::decode_image(&bytes, &sheet.image, &sheet.image).map_err(|e| e.to_string())
            })
            .map(|img| {
                let image =
                    egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
                ui.ctx().load_texture(
                    format!("ponko-palette-{}", sheet.image),
                    image,
                    egui::TextureOptions::NEAREST,
                )
            });
        match texture {
            Ok(t) => state.palette = Some((sheet.image.clone(), t)),
            Err(e) => {
                ui.colored_label(Color32::LIGHT_RED, e);
                return;
            }
        }
    }
    let Some((_, texture)) = &state.palette else { return };
    let [iw, ih] = texture.size().map(|n| n as f32);
    let scale = (240.0 / iw).clamp(0.25, 2.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(iw * scale, ih * scale), Sense::click());
    ui.data_mut(|d| d.insert_temp(palette_id(), rect));
    let painter = ui.painter_at(rect);
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let cell = |i: u32| {
        let (cx, cy) = (i % sheet.columns, i / sheet.columns);
        let x = sheet.margin + cx * (sheet.tile.0 + sheet.spacing);
        let y = sheet.margin + cy * (sheet.tile.1 + sheet.spacing);
        Rect::from_min_size(
            rect.min + egui::vec2(x as f32, y as f32) * scale,
            egui::vec2(sheet.tile.0 as f32, sheet.tile.1 as f32) * scale,
        )
    };
    painter.rect_stroke(
        cell(state.tile),
        0.0,
        Stroke::new(2.0, Color32::YELLOW),
        egui::StrokeKind::Inside,
    );
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(i) = (0..sheet.count).find(|&i| cell(i).contains(p))
    {
        state.tile = i;
    }
    ui.label(RichText::new(format!("tile {}", state.tile)).weak());
}

#[cfg(test)]
mod tests {
    use super::super::author_harness::*;
    use super::*;

    /// A tileset image of 8 × 4 tiles of 8 px (tile n has red = n) and an
    /// 6 × 4 map using it, in a folder of their own under `assets/`.
    struct Fixture {
        dir: std::path::PathBuf,
        map: String,
    }

    impl Fixture {
        fn new(what: &str) -> Self {
            let folder = format!("ponko-test-tilemap-{what}-{}", std::process::id());
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&folder);
            std::fs::create_dir_all(&dir).unwrap();
            let image =
                image::RgbaImage::from_fn(64, 32, |x, y| image::Rgba([((y / 8) * 8 + x / 8) as u8, 100, 100, 255]));
            image.save(dir.join("tiles.png")).unwrap();
            let map = json!({
                "type": "map", "orientation": "orthogonal", "renderorder": "right-down",
                "width": 6, "height": 4, "tilewidth": 8, "tileheight": 8, "infinite": false,
                "nextlayerid": 2, "nextobjectid": 1, "tiledversion": "1.10", "version": "1.10",
                "layers": [{"type": "tilelayer", "id": 1, "name": "ground", "width": 6, "height": 4,
                    "x": 0, "y": 0, "opacity": 1, "visible": true, "data": vec![0; 24]}],
                "tilesets": [{"firstgid": 1, "name": "tiles", "image": "tiles.png", "imagewidth": 64,
                    "imageheight": 32, "tilewidth": 8, "tileheight": 8, "columns": 8, "tilecount": 32,
                    "margin": 0, "spacing": 0}],
            });
            std::fs::write(dir.join("map.tmj"), serde_json::to_string_pretty(&map).unwrap()).unwrap();
            Self {
                dir,
                map: format!("{folder}/map.tmj"),
            }
        }

        fn on_disk(&self) -> Value {
            serde_json::from_str(&std::fs::read_to_string(self.dir.join("map.tmj")).unwrap()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// The editor with the fixture's level spawned at the origin, opened in
    /// the tilemap window through the Author menu.
    fn tile_editor(f: &Fixture) -> Harness {
        let mut h = Harness::new();
        h.ctx.spawn_level(&f.map, Vec2::ZERO).unwrap();
        // The map's middle in the free part of the view, clear of the
        // panels and the tilemap window.
        h.ctx.camera.zoom = 4.0;
        let middle = Vec2::new(24.0, 16.0);
        h.ctx.camera.position += middle - h.ctx.camera.screen_to_world(Vec2::new(850.0, 1100.0));
        h.click_text(|t| t == "Author");
        h.click_text(|t| t == "Tilemap…");
        h.frame(vec![]);
        h.type_text(&f.map);
        assert_eq!(
            h.panels.app.as_ref().unwrap().author.tilemap.path.as_deref(),
            Some(f.map.as_str()),
            "{:?}",
            h.panels.app.as_ref().unwrap().author.error
        );
        h
    }

    /// The middle of cell `(x, y)` on the screen.
    fn cell(h: &Harness, x: u32, y: u32) -> Vec2 {
        h.ctx
            .camera
            .world_to_screen(Vec2::new(x as f32 * 8.0 + 4.0, y as f32 * 8.0 + 4.0))
    }

    /// The tiles of the level's tile map in the world, row by row.
    fn world_tiles(h: &mut Harness) -> Vec<Option<u16>> {
        let maps: Vec<_> = h
            .ctx
            .world
            .query::<&crate::tilemap::TileMap>()
            .iter()
            .cloned()
            .collect();
        // An empty layer spawns no tile map.
        assert!(maps.len() <= 1, "{} tile maps", maps.len());
        let Some(m) = maps.first() else {
            return vec![None; 6 * 4];
        };
        (0..m.size.y)
            .flat_map(|y| (0..m.size.x).map(move |x| (x, y)))
            .map(|(x, y)| m.get(x, y))
            .collect()
    }

    fn doc(h: &Harness, f: &Fixture) -> Vec<u64> {
        h.ctx.editor.docs.get(&f.map).unwrap()["layers"][0]["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect()
    }

    #[test]
    fn a_tile_picked_from_the_palette_is_painted_where_clicked_and_undo_takes_it_back() {
        let f = Fixture::new("paint");
        let mut h = tile_editor(&f);
        // Tile 9 is the second row's second tile.
        let p = h.rect(palette_id());
        let scale = p.width() / 64.0;
        h.click(Vec2::new(p.left() + 12.0 * scale, p.top() + 12.0 * scale));
        assert_eq!(h.panels.app.as_ref().unwrap().author.tilemap.tile, 9);
        // Drag along the top row: one stroke.
        h.drag(cell(&h, 1, 0), cell(&h, 3, 0), 6);
        assert_eq!(&doc(&h, &f)[..6], &[0, 10, 10, 10, 0, 0]);
        let tiles = world_tiles(&mut h);
        assert_eq!(
            &tiles[..6],
            &[None, Some(9), Some(9), Some(9), None, None],
            "the level follows"
        );
        h.undo();
        assert!(doc(&h, &f).iter().all(|&g| g == 0), "the stroke was one step");
        h.frame(vec![]);
        assert!(world_tiles(&mut h).iter().all(Option::is_none));
        h.redo();
        assert_eq!(&doc(&h, &f)[..6], &[0, 10, 10, 10, 0, 0]);
    }

    #[test]
    fn ctrl_s_writes_the_painted_layer_into_the_tmj_and_keeps_the_rest() {
        let f = Fixture::new("save");
        let mut h = tile_editor(&f);
        h.click(cell(&h, 2, 1));
        assert!(h.ctx.editor.dirty(), "an unsaved map is an unsaved change");
        h.save();
        let disk = f.on_disk();
        assert_eq!(disk["layers"][0]["data"][8], 1);
        assert_eq!(disk["tilesets"][0]["image"], "tiles.png");
        assert_eq!(disk["tiledversion"], "1.10");
        assert!(!h.ctx.editor.docs.unsaved(&f.map));
        // The runtime loads what was saved.
        let level = h.ctx.assets.load_level(&f.map, &Default::default()).unwrap();
        assert!(!level.layers.is_empty());
    }

    #[test]
    fn the_autotile_brush_gives_each_cell_the_frame_its_neighbours_call_for() {
        let f = Fixture::new("auto");
        let mut h = tile_editor(&f);
        // A four-neighbour rule from tile 16 (the third row): frame 16 + mask.
        let mut map = h.ctx.editor.docs.get(&f.map).unwrap().clone();
        set_rule(&mut map["layers"][0], Some(("four".into(), 16)));
        h.ctx.editor.set_doc(&f.map, map);
        h.click_text(|t| t == "Autotile");
        h.drag(cell(&h, 1, 1), cell(&h, 3, 1), 6);
        let d = doc(&h, &f);
        // Left end: only E filled (mask 2); middle: E and W (10); right end: W (8).
        // Off the map counts as filled, but these cells are inside it.
        assert_eq!(&d[6..12], &[0, 17 + 2, 17 + 10, 17 + 8, 0, 0], "{d:?}");
        // Erasing the middle splits the row; its neighbours follow.
        h.click_text(|t| t == "Erase");
        h.click(cell(&h, 2, 1));
        assert_eq!(&doc(&h, &f)[6..12], &[0, 17, 0, 17, 0, 0]);
        h.save();
        let disk = f.on_disk();
        assert_eq!(disk["layers"][0]["properties"][0]["value"], "four 16");
    }

    #[test]
    fn a_base64_layer_opens_as_a_list_of_tile_numbers() {
        let mut layer = json!({"type": "tilelayer", "width": 2, "height": 1, "encoding": "base64",
            "data": "AQAAAAIAAAA="});
        let map = editable(json!({"layers": [layer.take()]}), "m.tmj").unwrap();
        assert_eq!(map["layers"][0]["data"], json!([1, 2]));
        assert!(map["layers"][0].get("encoding").is_none());
        assert!(editable(json!({"infinite": true, "layers": []}), "m.tmj").is_err());
    }
}
